use crate::options::MacroOpts;
use crate::signature::{self, TestInputSignature, TestReturnSignature};

use proc_macro2::{Span, TokenStream};
use quote::ToTokens;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::Token;
use syn::{
    AngleBracketedGenericArguments, AttrStyle, Attribute, Error, FnArg, GenericArgument,
    GenericParam, Generics, Ident, Item, ItemFn, ItemMod, Lifetime, ReturnType, Type,
    WherePredicate,
};

use std::collections::{HashMap, HashSet};

pub type FnArgs = Punctuated<FnArg, Token![,]>;

#[derive(Default)]
pub struct Tests {
    pub test_fns: Vec<TestFn>,
    pub input_sigs: HashMap<FnArgs, TestInputSignature>,
    pub return_sigs: HashMap<Box<Type>, TestReturnSignature>,
}

pub struct TestFn {
    pub test_attrs: Vec<Attribute>,
    pub asyncness: Option<Token![async]>,
    pub unsafety: Option<Token![unsafe]>,
    pub ident: Ident,
    pub lifetime_params: Punctuated<Lifetime, Token![,]>,
    pub inputs: FnArgs,
    pub output: ReturnType,
}

impl Tests {
    pub fn try_extract<'ast>(
        opts: &MacroOpts,
        ast: &'ast mut ItemMod,
    ) -> syn::Result<(Self, &'ast mut Vec<Item>)> {
        let span = ast.span();
        let items = match ast.content.as_mut() {
            Some(content) => &mut content.1,
            None => return Err(Error::new(span, "only inline modules are supported")),
        };
        let mut mod_wide_generic_arity = None;
        let mut tests = Tests::default();
        for item in items.iter_mut() {
            if let Item::Fn(item) = item {
                if tests.try_extract_fn(opts, item)? {
                    let fn_generic_arity = generic_arity(&item.sig.generics);
                    match mod_wide_generic_arity {
                        None => {
                            mod_wide_generic_arity = Some(fn_generic_arity);
                        }
                        Some(n) => {
                            if fn_generic_arity != n {
                                return Err(Error::new_spanned(
                                    &*item,
                                    format!(
                                        "test function `{}` has {} generic parameters \
                                        while others in the same module have {}",
                                        item.sig.ident, fn_generic_arity, n
                                    ),
                                ));
                            }
                        }
                    }
                }
            }
        }
        Ok((tests, items))
    }

    fn try_extract_fn(&mut self, opts: &MacroOpts, item: &mut ItemFn) -> syn::Result<bool> {
        let test_attrs = extract_test_attrs(opts, item);
        if test_attrs.is_empty() {
            return Ok(false);
        }
        signature::validate(&item.sig)?;
        let inputs = item.sig.inputs.clone();
        let mut lifetimes = if inputs.is_empty() {
            HashSet::new()
        } else if let Some(sig) = self.input_sigs.get(&inputs) {
            sig.item.lifetimes.clone()
        } else {
            let sig_ident = Ident::new(
                &format!("_generic_tests_Args{}", self.input_sigs.len()),
                Span::call_site(),
            );
            let sig = TestInputSignature::try_build(sig_ident, &inputs)?;
            let lifetimes = sig.item.lifetimes.clone();
            self.input_sigs.insert(inputs.clone(), sig);
            lifetimes
        };
        let output = item.sig.output.clone();
        match &output {
            ReturnType::Default => {}
            ReturnType::Type(_, ty) => {
                if let Some(sig) = self.return_sigs.get(ty) {
                    lifetimes = lifetimes.union(&sig.item.lifetimes).cloned().collect();
                } else {
                    let ret_ident = Ident::new(
                        &format!("_generic_tests_Ret{}", self.return_sigs.len()),
                        Span::call_site(),
                    );
                    let input_lifetimes = if inputs.is_empty() {
                        None
                    } else {
                        self.input_sigs.get(&inputs).map(|sig| &sig.item.lifetimes)
                    };
                    let sig = TestReturnSignature::try_build(ret_ident, &ty, input_lifetimes)?;
                    lifetimes = lifetimes.union(&sig.item.lifetimes).cloned().collect();
                    self.return_sigs.insert(ty.clone(), sig);
                }
            }
        }
        let lifetime_params = filter_lifetime_params(&item.sig.generics, &lifetimes)?;
        self.test_fns.push(TestFn {
            test_attrs,
            asyncness: item.sig.asyncness,
            unsafety: item.sig.unsafety,
            ident: item.sig.ident.clone(),
            lifetime_params,
            inputs,
            output,
        });
        Ok(true)
    }
}

fn extract_test_attrs(opts: &MacroOpts, item: &mut ItemFn) -> Vec<Attribute> {
    let mut test_attrs = Vec::new();
    let mut pos = 0;
    while pos < item.attrs.len() {
        let attr = &item.attrs[pos];
        if opts.is_test_attr(&attr) {
            test_attrs.push(item.attrs.remove(pos));
            continue;
        }
        pos += 1;
    }
    if !test_attrs.is_empty() {
        for attr in &item.attrs {
            if opts.is_copied_attr(&attr) {
                test_attrs.push(attr.clone());
            }
        }
    }
    test_attrs
}

fn generic_arity(generics: &Generics) -> usize {
    generics
        .params
        .iter()
        .filter(|param| match param {
            GenericParam::Type(_) | GenericParam::Const(_) => true,
            GenericParam::Lifetime(_) => false,
        })
        .count()
}

fn filter_lifetime_params(
    generics: &Generics,
    lifetimes_used: &HashSet<Lifetime>,
) -> syn::Result<Punctuated<Lifetime, Token![,]>> {
    fn validate_lifetime_def<'a>(
        lifetime: &'a Lifetime,
        bounds: &Punctuated<Lifetime, Token![+]>,
    ) -> syn::Result<&'a Lifetime> {
        if !bounds.is_empty() {
            return Err(Error::new_spanned(
                bounds,
                "lifetime bounds are not supported in generic test functions",
            ));
        }
        Ok(lifetime)
    }

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
    let params = generics
        .lifetimes()
        .filter(|def| lifetimes_used.contains(&def.lifetime))
        .map(|def| validate_lifetime_def(&def.lifetime, &def.bounds).map(Clone::clone))
        .collect::<syn::Result<_>>()?;
    Ok(params)
}

pub struct InstArguments(Punctuated<GenericArgument, Token![,]>);

impl InstArguments {
    pub fn try_extract(item: &mut ItemMod) -> syn::Result<Option<Self>> {
        for (pos, attr) in item.attrs.iter().enumerate() {
            if attr.path.is_ident("instantiate_tests") {
                match attr.style {
                    AttrStyle::Outer => {}
                    AttrStyle::Inner(_) => {
                        return Err(Error::new_spanned(attr, "cannot be an inner attribute"))
                    }
                };
                let AngleBracketedGenericArguments { args, .. } = attr.parse_args()?;
                item.attrs.remove(pos);
                return Ok(Some(InstArguments(args)));
            }
        }
        Ok(None)
    }
}

impl ToTokens for InstArguments {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.to_tokens(tokens)
    }
}
