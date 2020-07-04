use crate::error::ErrorRecord;

use proc_macro2::Span;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::visit_mut::{self, VisitMut};
use syn::{parse_quote, Token};
use syn::{
    BoundLifetimes, ConstParam, Error, FnArg, GenericParam, Generics, Ident, ItemFn, Lifetime,
    ParenthesizedGenericArguments, Pat, PatIdent, Path, PathSegment, ReturnType, Signature,
    TraitBound, Type, TypeBareFn, TypeParam, TypeReference, WherePredicate,
};

use std::collections::HashSet;
use std::mem;

pub struct TestFnSignature {
    pub input: TestInputSignature,
    pub output: TestReturnSignature,
    pub lifetime_params: Punctuated<Lifetime, Token![,]>,
}

pub struct TestSignatureItem {
    // We don't care about the order in which the lifetime parameters/arguments
    // are listed, as long as it is consistent between all places where
    // they are enumerated during the macro's invocation.
    // It should be so once the signature is complete and is not mutated.
    pub lifetimes: HashSet<Lifetime>,
}

pub struct TestInputSignature {
    pub item: TestSignatureItem,
    pub args: Vec<TestFnArg>,
}

pub struct TestFnArg {
    pub ident: Ident,
    pub ty: Box<Type>,
}

pub struct TestReturnSignature {
    pub item: TestSignatureItem,
    pub ty: Box<Type>,
}

impl TestSignatureItem {
    pub fn lifetime_generics(&self) -> Generics {
        let lifetimes = self.lifetimes.iter();
        parse_quote! { <#(#lifetimes),*> }
    }

    pub fn path_segment(&self, name: &str) -> PathSegment {
        let ident = Ident::new(name, Span::call_site());
        if self.lifetimes.is_empty() {
            parse_quote! { #ident }
        } else {
            let lifetimes = self.lifetimes.iter();
            parse_quote! { #ident<#(#lifetimes),*> }
        }
    }
}

impl TestFnSignature {
    pub fn try_build(item: &ItemFn) -> syn::Result<Self> {
        validate(&item.sig)?;
        let input = TestInputSignature::try_build(&item.sig.inputs)?;
        let mut lifetimes = input.item.lifetimes.clone();
        let output = match &item.sig.output {
            ReturnType::Default => TestReturnSignature::default(),
            ReturnType::Type(_, ty) => {
                let sig = TestReturnSignature::try_build(&ty, &input.item.lifetimes)?;
                lifetimes = lifetimes.union(&sig.item.lifetimes).cloned().collect();
                sig
            }
        };
        let lifetime_params = filter_fn_lifetimes(&item.sig.generics, &lifetimes)?;
        Ok(TestFnSignature {
            input,
            output,
            lifetime_params,
        })
    }
}

impl TestInputSignature {
    fn try_build<'a>(inputs: impl IntoIterator<Item = &'a FnArg>) -> syn::Result<Self> {
        let mut lifetime_collector = LifetimeCollector::new(LifetimeSubstMode::Input);
        let args = inputs
            .into_iter()
            .map(|input| match input {
                FnArg::Typed(type_pat) => match &*type_pat.pat {
                    Pat::Ident(PatIdent { ident, .. }) => {
                        let mut ty = type_pat.ty.clone();
                        lifetime_collector.visit_type_mut(&mut ty);
                        Ok(TestFnArg {
                            ident: ident.clone(),
                            ty,
                        })
                    }
                    Pat::Wild(wild) => Err(Error::new_spanned(
                        wild,
                        "wildcard pattern not allowed in generic test function input",
                    )),
                    _ => Err(Error::new_spanned(
                        type_pat,
                        "unsupported argument pattern in test function input",
                    )),
                },
                FnArg::Receiver(_) => Err(Error::new_spanned(
                    input,
                    "unexpected receiver argument in a test function",
                )),
            })
            .collect::<syn::Result<_>>()?;
        let lifetimes = lifetime_collector.validate()?;
        Ok(TestInputSignature {
            item: TestSignatureItem { lifetimes },
            args,
        })
    }
}

impl Default for TestReturnSignature {
    fn default() -> Self {
        TestReturnSignature {
            item: TestSignatureItem {
                lifetimes: Default::default(),
            },
            ty: Box::new(parse_quote! { () }),
        }
    }
}

impl TestReturnSignature {
    fn try_build(ty: &Type, input_lifetimes: &HashSet<Lifetime>) -> syn::Result<Self> {
        use LifetimeSubstMode as Mode;

        let subst_mode = {
            let mut iter = input_lifetimes.iter();
            iter.next().map(|lifetime| {
                if iter.len() == 0 {
                    Mode::Output(lifetime.clone())
                } else {
                    Mode::Fail
                }
            })
        }
        .unwrap_or(Mode::Fail);
        let mut lifetime_collector = LifetimeCollector::new(subst_mode);
        let mut ty = Box::new(ty.clone());
        lifetime_collector.visit_type_mut(&mut ty);
        let lifetimes = lifetime_collector.validate()?;
        Ok(TestReturnSignature {
            item: TestSignatureItem { lifetimes },
            ty,
        })
    }
}

enum LifetimeSubstMode {
    Disabled,
    Input,
    Output(Lifetime),
    Fail,
}

// Visits type signatures to collect lifetimes used,
// generate names for elided lifetimes, and substitute uses of the lifetime
// placeholder with the actual lifetime (if it is found to be unique).
struct LifetimeCollector {
    lifetimes: HashSet<Lifetime>,
    subst_mode: LifetimeSubstMode,
    bound_lifetimes: HashSet<Lifetime>,
    placeholder_lifetime_at: Option<Span>,
    errors: ErrorRecord,
}

impl LifetimeCollector {
    fn new(subst_mode: LifetimeSubstMode) -> Self {
        LifetimeCollector {
            lifetimes: HashSet::new(),
            subst_mode,
            bound_lifetimes: HashSet::new(),
            placeholder_lifetime_at: None,
            errors: Default::default(),
        }
    }

    fn collect_lifetime(&mut self, lifetime: &Lifetime) {
        if !self.lifetimes.contains(lifetime) && !self.bound_lifetimes.contains(lifetime) {
            self.lifetimes.insert(lifetime.clone());
        }
    }

    fn add_elided_lifetime(&mut self) -> Lifetime {
        let symbol = format!("'_generic_tests_{}", self.lifetimes.len());
        let lifetime = Lifetime::new(&symbol, Span::call_site());
        let is_unique = self.lifetimes.insert(lifetime.clone());
        assert!(
            is_unique,
            "lifetime {} is already present; \
            `'_generic_tests_*` lifetimes are reserved for macro use",
            lifetime,
        );
        lifetime
    }

    fn subst_placeholder_lifetime(&mut self, placeholder: &mut Lifetime) {
        use LifetimeSubstMode as Mode;

        if let Mode::Disabled = self.subst_mode {
            return;
        }
        if !self.bound_lifetimes.is_empty() {
            self.errors.add_error(Error::new_spanned(
                placeholder,
                "can't determine the lifetime this placeholder refers to \
                in presence of bound lifetime parameters",
            ));
            return;
        }
        let (lifetime, injected) = if let Some(lifetime) = self.lifetimes.iter().next() {
            (lifetime, false)
        } else {
            match &self.subst_mode {
                Mode::Input => {
                    self.errors.add_error(Error::new_spanned(
                        placeholder,
                        "can't determine the lifetime this placeholder refers to",
                    ));
                    return;
                }
                Mode::Output(lifetime) => (lifetime, true),
                Mode::Fail => {
                    self.errors.add_error(Error::new_spanned(
                        placeholder,
                        "lifetime needs to be disambiguated",
                    ));
                    return;
                }
                Mode::Disabled => unreachable!(),
            }
        };
        placeholder.ident = lifetime.ident.clone();
        if injected {
            self.collect_lifetime(&placeholder);
        }
        self.placeholder_lifetime_at = Some(placeholder.span());
    }

    fn validate(mut self) -> syn::Result<HashSet<Lifetime>> {
        if let Some(span) = self.placeholder_lifetime_at {
            if self.lifetimes.len() > 1 {
                self.errors
                    .add_error(Error::new(span, "lifetime needs to be disambiguated"));
            }
        }
        self.errors.check()?;
        Ok(self.lifetimes)
    }
}

impl VisitMut for LifetimeCollector {
    fn visit_lifetime_mut(&mut self, lifetime: &mut Lifetime) {
        if lifetime.ident == "static" {
            return;
        }
        if lifetime.ident == "_" {
            self.subst_placeholder_lifetime(lifetime);
        } else {
            self.collect_lifetime(lifetime);
        }
    }

    fn visit_type_reference_mut(&mut self, ref_type: &mut TypeReference) {
        use LifetimeSubstMode as Mode;

        match &mut ref_type.lifetime {
            Some(lifetime) => {
                self.visit_lifetime_mut(lifetime);
            }
            None => match &self.subst_mode {
                Mode::Disabled => {}
                Mode::Input => {
                    let lifetime = self.add_elided_lifetime();
                    ref_type.lifetime = Some(lifetime);
                }
                Mode::Output(lifetime) => {
                    let lifetime = lifetime.clone();
                    self.collect_lifetime(&lifetime);
                    ref_type.lifetime = Some(lifetime);
                }
                Mode::Fail => {
                    self.errors.add_error(Error::new_spanned(
                        ref_type,
                        "elided reference lifetime needs to be disambiguated",
                    ));
                    return;
                }
            },
        }
        visit_mut::visit_type_mut(self, &mut ref_type.elem)
    }

    fn visit_type_bare_fn_mut(&mut self, fn_type: &mut TypeBareFn) {
        // A function pointer type forms its own lifetime inference context
        let mut suppression = LifetimeInferenceSuppression::new(self);
        let mut scope =
            LifetimeBindingScope::new(suppression.visitor_mut(), fn_type.lifetimes.as_ref());
        let this = scope.visitor_mut();
        visit_mut::visit_type_bare_fn_mut(this, fn_type)
    }

    fn visit_trait_bound_mut(&mut self, bound: &mut TraitBound) {
        let mut scope = LifetimeBindingScope::new(self, bound.lifetimes.as_ref());
        let this = scope.visitor_mut();
        visit_mut::visit_trait_bound_mut(this, bound)
    }

    fn visit_parenthesized_generic_arguments_mut(
        &mut self,
        args: &mut ParenthesizedGenericArguments,
    ) {
        // A closure trait signature forms its own lifetime inference context
        let mut suppression = LifetimeInferenceSuppression::new(self);
        let this = suppression.visitor_mut();
        visit_mut::visit_parenthesized_generic_arguments_mut(this, args)
    }
}

#[must_use = "should be assigned to a local variable"]
struct LifetimeInferenceSuppression<'a> {
    visitor: &'a mut LifetimeCollector,
    outer_mode: LifetimeSubstMode,
}

impl<'a> LifetimeInferenceSuppression<'a> {
    fn new(visitor: &'a mut LifetimeCollector) -> Self {
        let outer_mode = mem::replace(&mut visitor.subst_mode, LifetimeSubstMode::Disabled);
        LifetimeInferenceSuppression {
            visitor,
            outer_mode,
        }
    }

    fn visitor_mut(&mut self) -> &mut LifetimeCollector {
        &mut self.visitor
    }
}

impl<'a> Drop for LifetimeInferenceSuppression<'a> {
    fn drop(&mut self) {
        self.visitor.subst_mode = mem::replace(&mut self.outer_mode, LifetimeSubstMode::Disabled);
    }
}

#[must_use = "should be assigned to a local variable"]
struct LifetimeBindingScope<'a> {
    visitor: &'a mut LifetimeCollector,
    outer_bindings: Option<HashSet<Lifetime>>,
}

impl<'a> LifetimeBindingScope<'a> {
    fn new(visitor: &'a mut LifetimeCollector, binding: Option<&BoundLifetimes>) -> Self {
        let outer_bindings = binding.map(|binding| {
            let mut bound_lifetimes = visitor.bound_lifetimes.clone();
            for def in &binding.lifetimes {
                bound_lifetimes.insert(def.lifetime.clone());
            }
            mem::replace(&mut visitor.bound_lifetimes, bound_lifetimes)
        });
        LifetimeBindingScope {
            visitor,
            outer_bindings,
        }
    }

    fn visitor_mut(&mut self) -> &mut LifetimeCollector {
        &mut self.visitor
    }
}

impl<'a> Drop for LifetimeBindingScope<'a> {
    fn drop(&mut self) {
        if let Some(bound_lifetimes) = self.outer_bindings.take() {
            self.visitor.bound_lifetimes = bound_lifetimes;
        }
    }
}

// Checks for any uses of generic type and const parameters and reports
// an error if found, as this macro can not yet substitute these parameters
// in test function signatures.
struct GenericParamCatcher {
    generic_params: HashSet<Ident>,
    errors: ErrorRecord,
}

impl GenericParamCatcher {
    fn new(generics: &Generics) -> Self {
        let generic_params = generics
            .params
            .iter()
            .filter_map(|param| match param {
                GenericParam::Type(TypeParam { ident, .. }) => Some(ident.clone()),
                GenericParam::Const(ConstParam { ident, .. }) => Some(ident.clone()),
                GenericParam::Lifetime(_) => None,
            })
            .collect();
        GenericParamCatcher {
            generic_params,
            errors: Default::default(),
        }
    }
}

impl<'ast> Visit<'ast> for GenericParamCatcher {
    fn visit_path(&mut self, path: &'ast Path) {
        if let Some(ident) = path.get_ident() {
            if self.generic_params.contains(ident) {
                self.errors.add_error(Error::new_spanned(
                    ident,
                    "use of generic parameters in test function signatures is not supported",
                ));
            }
        } else {
            visit::visit_path(self, path)
        }
    }
}

fn validate(sig: &Signature) -> syn::Result<()> {
    if sig.constness.is_some() {
        return Err(Error::new_spanned(
            &sig.constness,
            "generic test function cannot be const",
        ));
    }
    if sig.abi.is_some() {
        return Err(Error::new_spanned(
            &sig.abi,
            "extern ABI is not supported in a generic test function",
        ));
    }
    if sig.variadic.is_some() {
        return Err(Error::new_spanned(
            &sig.variadic,
            "variadic arguments are not supported in a generic test function",
        ));
    }
    let mut catcher = GenericParamCatcher::new(&sig.generics);
    for arg in &sig.inputs {
        catcher.visit_fn_arg(arg);
    }
    match &sig.output {
        ReturnType::Default => {}
        ReturnType::Type(_, ty) => catcher.visit_type(&ty),
    }
    catcher.errors.check()
}

fn filter_fn_lifetimes(
    generics: &Generics,
    lifetimes_used: &HashSet<Lifetime>,
) -> syn::Result<Punctuated<Lifetime, Token![,]>> {
    let lifetimes = generics
        .lifetimes()
        .filter(|def| lifetimes_used.contains(&def.lifetime))
        .map(|def| validate_lifetime_def(&def.lifetime, &def.bounds).map(|()| def.lifetime.clone()))
        .collect::<syn::Result<_>>()?;
    if let Some(where_clause) = &generics.where_clause {
        for predicate in &where_clause.predicates {
            match predicate {
                WherePredicate::Lifetime(predicate) => {
                    if lifetimes_used.contains(&predicate.lifetime) {
                        validate_lifetime_def(&predicate.lifetime, &predicate.bounds)?;
                    }
                }
                WherePredicate::Type(_) | WherePredicate::Eq(_) => {}
            }
        }
    }
    Ok(lifetimes)
}

fn validate_lifetime_def<'ast>(
    _: &'ast Lifetime,
    bounds: &'ast Punctuated<Lifetime, Token![+]>,
) -> syn::Result<()> {
    if !bounds.is_empty() {
        return Err(Error::new_spanned(
            bounds,
            "lifetime bounds are not supported in generic test functions",
        ));
    }
    Ok(())
}
