use crate::error::ErrorRecord;

use proc_macro2::Span;
use syn::punctuated::Punctuated;
use syn::visit::{self, Visit};
use syn::visit_mut::{self, VisitMut};
use syn::{parse_quote, Token};
use syn::{
    Attribute, BoundLifetimes, ConstParam, Error, FnArg, GenericParam, Generics, Ident, ItemFn,
    Lifetime, ParenthesizedGenericArguments, Pat, PatIdent, Path, PathSegment, ReturnType,
    Signature, TraitBound, Type, TypeBareFn, TypeParam, TypePath, TypeReference, WherePredicate,
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
    pub attrs: Vec<Attribute>,
    pub ident: Ident,
    // Argument type as in the original function
    pub arg_ty: Box<Type>,
    // Type with all lifetimes made explicit for the arg structure field
    pub field_ty: Box<Type>,
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

impl TestFnArg {
    pub fn to_fn_arg(&self) -> FnArg {
        let attrs = self.attrs.iter();
        let ident = &self.ident;
        let ty = &*self.arg_ty;
        parse_quote! {
            #(#attrs)* #ident: #ty
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
                let sig = TestReturnSignature::try_build(ty, &input.item.lifetimes)?;
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
                FnArg::Typed(arg) => match &*arg.pat {
                    Pat::Ident(PatIdent {
                        ident,
                        mutability: _,
                        attrs,
                        by_ref,
                        subpat,
                    }) => {
                        if by_ref.is_some() || subpat.is_some() || !attrs.is_empty() {
                            return Err(Error::new_spanned(
                                &arg.pat,
                                "unsupported features in an argument pattern",
                            ));
                        }
                        let arg_ty = arg.ty.clone();
                        let mut field_ty = arg_ty.clone();
                        lifetime_collector.visit_type_mut(&mut field_ty);
                        Ok(TestFnArg {
                            attrs: arg.attrs.clone(),
                            ident: ident.clone(),
                            arg_ty,
                            field_ty,
                        })
                    }
                    Pat::Wild(wild) => Err(Error::new_spanned(
                        wild,
                        "wildcard pattern not allowed in generic test function input",
                    )),
                    _ => Err(Error::new_spanned(
                        arg,
                        "unsupported argument pattern in generic test function input",
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
    errors: ErrorRecord,
}

impl LifetimeCollector {
    fn new(subst_mode: LifetimeSubstMode) -> Self {
        LifetimeCollector {
            lifetimes: HashSet::new(),
            subst_mode,
            bound_lifetimes: HashSet::new(),
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

        let lifetime = match &self.subst_mode {
            Mode::Disabled => return,
            Mode::Output(lifetime) => lifetime,
            Mode::Input | Mode::Fail => {
                self.errors.add_error(Error::new_spanned(
                    placeholder,
                    "lifetime needs to be disambiguated",
                ));
                return;
            }
        };
        placeholder.ident = lifetime.ident.clone();
        self.collect_lifetime(placeholder);
    }

    fn validate(self) -> syn::Result<HashSet<Lifetime>> {
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
        self.visitor
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
            for param in &binding.lifetimes {
                match param {
                    GenericParam::Lifetime(def) => {
                        bound_lifetimes.insert(def.lifetime.clone());
                    }
                    _ => panic!("unexpected generic parameter in bound lifetimes"),
                }
            }
            mem::replace(&mut visitor.bound_lifetimes, bound_lifetimes)
        });
        LifetimeBindingScope {
            visitor,
            outer_bindings,
        }
    }

    fn visitor_mut(&mut self) -> &mut LifetimeCollector {
        self.visitor
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
        const ERROR_MSG: &str =
            "use of generic parameters in test function signatures is not supported";

        if let Some(ident) = path.get_ident() {
            if self.generic_params.contains(ident) {
                self.errors.add_error(Error::new_spanned(ident, ERROR_MSG));
            }
            return;
        }
        if path.leading_colon.is_none() && path.segments.len() == 2 {
            use syn::PathArguments::*;
            if let (None, None) = (&path.segments[0].arguments, &path.segments[1].arguments) {
                let suspected_param = &path.segments[0].ident;
                if self.generic_params.contains(suspected_param) {
                    self.errors
                        .add_error(Error::new_spanned(suspected_param, ERROR_MSG));
                }
                return;
            }
        }
        visit::visit_path(self, path)
    }

    fn visit_type_path(&mut self, type_path: &'ast TypePath) {
        match &type_path.qself {
            None => self.visit_path(&type_path.path),
            Some(qself) => self.visit_qself(qself),
        }
    }
}

fn validate(sig: &Signature) -> syn::Result<()> {
    if sig.constness.is_some() {
        return Err(Error::new_spanned(
            sig.constness,
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
        ReturnType::Type(_, ty) => catcher.visit_type(ty),
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
                _ => {}
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
