use crate::error::ErrorRecord;
use crate::options::{self, MacroOpts, TestFnOpts};
use crate::signature::TestFnSignature;

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::punctuated::Punctuated;
use syn::Token;
use syn::{
    AngleBracketedGenericArguments, AttrStyle, Attribute, Error, GenericArgument, GenericParam,
    Generics, Ident, Item, ItemFn, ItemMod, ReturnType,
};

#[derive(Default)]
pub struct Tests {
    pub test_fns: Vec<TestFn>,
}

pub struct TestFn {
    pub test_attrs: Vec<Attribute>,
    pub asyncness: Option<Token![async]>,
    pub unsafety: Option<Token![unsafe]>,
    pub ident: Ident,
    pub output: ReturnType,
    pub sig: TestFnSignature,
}

impl Tests {
    pub fn try_extract<'ast>(
        opts: &MacroOpts,
        ast: &'ast mut ItemMod,
    ) -> syn::Result<(Self, &'ast mut Vec<Item>)> {
        if ast.content.is_none() {
            return Err(Error::new_spanned(ast, "only inline modules are supported"));
        }
        let items = &mut ast.content.as_mut().unwrap().1;
        let (tests, errors) = Self::extract_recording_errors(opts, items);
        errors.check()?;
        Ok((tests, items))
    }

    fn extract_recording_errors(opts: &MacroOpts, items: &mut [Item]) -> (Self, ErrorRecord) {
        let mut errors = ErrorRecord::default();
        let mut tests = Tests::default();
        let mut mod_wide_generic_arity = None;
        for item in items.iter_mut() {
            if let Item::Fn(item) = item {
                match TestFn::try_extract(opts, item) {
                    Ok(None) => {}
                    Ok(Some(test_fn)) => {
                        let fn_generic_arity = generic_arity(&item.sig.generics);
                        match mod_wide_generic_arity {
                            None => {
                                mod_wide_generic_arity = Some(fn_generic_arity);
                            }
                            Some(n) => {
                                if fn_generic_arity != n {
                                    errors.add_error(Error::new_spanned(
                                        &item.sig.generics,
                                        format!(
                                            "test function `{}` has {} generic parameters \
                                            while others in the same module have {}",
                                            item.sig.ident, fn_generic_arity, n
                                        ),
                                    ));
                                    continue;
                                }
                            }
                        }
                        tests.test_fns.push(test_fn);
                    }
                    Err(e) => {
                        errors.add_error(e);
                        continue;
                    }
                }
            }
        }
        (tests, errors)
    }
}

impl TestFn {
    fn try_extract(opts: &MacroOpts, item: &mut ItemFn) -> syn::Result<Option<Self>> {
        let test_attrs = extract_test_attrs(opts, item)?;
        if test_attrs.is_empty() {
            return Ok(None);
        }
        let sig = TestFnSignature::try_build(item)?;
        Ok(Some(TestFn {
            test_attrs,
            asyncness: item.sig.asyncness,
            unsafety: item.sig.unsafety,
            ident: item.sig.ident.clone(),
            output: item.sig.output.clone(),
            sig,
        }))
    }
}

fn extract_test_attrs(opts: &MacroOpts, item: &mut ItemFn) -> syn::Result<Vec<Attribute>> {
    let mut fn_opts = TestFnOpts::default();
    let mut pos = 0;
    while pos < item.attrs.len() {
        let attr = &item.attrs[pos];
        if attr.meta.path().is_ident("generic_test") {
            let attr = item.attrs.remove(pos);
            fn_opts.apply_attr(attr.meta)?;
            continue;
        }
        pos += 1;
    }
    let mut test_attrs = Vec::new();
    let mut pos = 0;
    while pos < item.attrs.len() {
        let attr = &item.attrs[pos];
        if options::is_test_attr(attr, opts, &fn_opts) {
            test_attrs.push(item.attrs.remove(pos));
            continue;
        }
        pos += 1;
    }
    if !test_attrs.is_empty() {
        for attr in &item.attrs {
            if options::is_copied_attr(attr, opts, &fn_opts) {
                test_attrs.push(attr.clone());
            }
        }
    }
    Ok(test_attrs)
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

pub struct InstArguments(Punctuated<GenericArgument, Token![,]>);

impl InstArguments {
    pub fn try_extract(item: &mut ItemMod) -> syn::Result<Option<Self>> {
        for (pos, attr) in item.attrs.iter().enumerate() {
            if attr.meta.path().is_ident("instantiate_tests") {
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
