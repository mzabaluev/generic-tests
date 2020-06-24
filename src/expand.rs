use proc_macro2::{Span, TokenStream};
use quote::ToTokens;
use quote::{quote, quote_spanned};
use syn::parse_quote;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit_mut::{self, VisitMut};
use syn::Token;
use syn::{
    AngleBracketedGenericArguments, AttrStyle, Attribute, Error, FnArg, GenericArgument, Ident,
    Item, ItemFn, ItemMod, Pat, PatIdent, ReturnType,
};

const TEST_ATTRS: &[&str] = &["test", "ignore", "should_panic", "bench"];
const COPIED_ATTRS: &[&str] = &["cfg"];

pub fn expand(mut ast: ItemMod) -> TokenStream {
    match transform(&mut ast) {
        Ok(()) => ast.into_token_stream(),
        Err(e) => e.to_compile_error(),
    }
}

fn transform(ast: &mut ItemMod) -> syn::Result<()> {
    let tests = extract_test_fns(ast)?;
    instantiate(tests, ast)
}

type TestFnArgs = Punctuated<Ident, Token![,]>;

struct TestFn {
    test_attrs: Vec<Attribute>,
    name: Ident,
    inputs: Punctuated<FnArg, Token![,]>,
    output: ReturnType,
    args: TestFnArgs,
}

fn extract_test_attrs(item: &mut ItemFn) -> Vec<Attribute> {
    let mut test_attrs = Vec::new();
    let mut pos = 0;
    while pos < item.attrs.len() {
        let attr = &item.attrs[pos];
        if TEST_ATTRS.iter().any(|name| attr.path.is_ident(name)) {
            test_attrs.push(item.attrs.remove(pos))
        } else {
            pos += 1;
        }
    }
    if !test_attrs.is_empty() {
        for attr in &item.attrs {
            if COPIED_ATTRS.iter().any(|name| attr.path.is_ident(name)) {
                test_attrs.push(attr.clone());
            }
        }
    }
    test_attrs
}

impl TestFn {
    fn try_extract(item: &mut ItemFn) -> syn::Result<Option<Self>> {
        let test_attrs = extract_test_attrs(item);
        if !test_attrs.is_empty() {
            let args = item
                .sig
                .inputs
                .iter()
                .map(|input| match input {
                    FnArg::Typed(type_pat) => match &*type_pat.pat {
                        Pat::Ident(PatIdent { ident, .. }) => Ok(ident.clone()),
                        _ => Err(Error::new_spanned(
                            type_pat,
                            "unsupported pattern in test function input",
                        )),
                    },
                    FnArg::Receiver(_) => Err(Error::new_spanned(
                        input,
                        "unexpected receiver argument in a test function",
                    )),
                })
                .collect::<syn::Result<TestFnArgs>>()?;
            return Ok(Some(TestFn {
                test_attrs,
                name: item.sig.ident.clone(),
                inputs: item.sig.inputs.clone(),
                output: item.sig.output.clone(),
                args,
            }));
        }
        Ok(None)
    }
}

fn extract_test_fns(ast: &mut ItemMod) -> syn::Result<Vec<TestFn>> {
    let span = ast.span();
    let items = match ast.content.as_mut() {
        Some(content) => &mut content.1,
        None => return Err(Error::new(span, "only inline modules are supported")),
    };
    let mut test_fns = Vec::new();
    for item in items {
        if let Item::Fn(item) = item {
            if let Some(test_fn) = TestFn::try_extract(item)? {
                test_fns.push(test_fn)
            }
        }
    }
    Ok(test_fns)
}

struct InstArguments {
    args: Punctuated<GenericArgument, Token![,]>,
    attr_span: Span,
}

impl InstArguments {
    fn try_extract(item: &mut ItemMod) -> syn::Result<Option<Self>> {
        for (pos, attr) in item.attrs.iter().enumerate() {
            if attr.path.is_ident("instantiate_tests") {
                match attr.style {
                    AttrStyle::Outer => {}
                    AttrStyle::Inner(_) => {
                        return Err(Error::new_spanned(attr, "cannot be an inner attribute"))
                    }
                };
                let AngleBracketedGenericArguments { args, .. } = attr.parse_args()?;
                let attr_span = attr.span();
                item.attrs.remove(pos);
                return Ok(Some(InstArguments { args, attr_span }));
            }
        }
        Ok(None)
    }
}

struct Instantiator {
    tests: Vec<TestFn>,
    depth: u32,
    error: Option<Error>,
}

fn instantiate(tests: Vec<TestFn>, ast: &mut ItemMod) -> syn::Result<()> {
    let mut instantiator = Instantiator {
        tests,
        depth: 1,
        error: None,
    };
    visit_mut::visit_item_mod_mut(&mut instantiator, ast);
    match instantiator.error {
        None => Ok(()),
        Some(e) => Err(e),
    }
}

impl Instantiator {
    fn record_error(&mut self, error: Error) {
        match &mut self.error {
            None => {
                self.error = Some(error);
            }
            Some(existing) => existing.combine(error),
        }
    }

    fn instantiate_tests(&self, inst_args: InstArguments, content: &mut Vec<Item>) {
        debug_assert!(content.is_empty());

        // Get path prefix to the macro invocation's root module.
        let mut super_prefix = TokenStream::new();
        for _ in 0..self.depth {
            super_prefix.extend(quote! {super::});
        }

        // The order of glob imports is important. If identifiers in the parent
        // module scope alias those of the root module, we don't want lints on
        // identifiers that are actually unused in the parent, but used in the
        // instantiation arguments. So import the names from the parent first.
        content.push(
            syn::parse2(quote_spanned! { inst_args.attr_span =>
                #[allow(unused_imports)]
                use super::*;
            })
            .unwrap(),
        );
        if self.depth > 1 {
            content.push(parse_quote! {
                #[allow(unused_imports)]
                use #super_prefix*;
            });
        }

        for test in &self.tests {
            let test_attrs = &test.test_attrs;
            let name = &test.name;
            let inputs = &test.inputs;
            let output = &test.output;
            let generic_args = &inst_args.args;
            let fn_args = &test.args;
            content.push(parse_quote! {
                #(#test_attrs)*
                fn #name(#inputs) #output {
                    #super_prefix#name::<#generic_args>(#fn_args)
                }
            })
        }
    }
}

impl VisitMut for Instantiator {
    fn visit_item_mod_mut(&mut self, item: &mut ItemMod) {
        debug_assert_ne!(self.depth, 0);
        match InstArguments::try_extract(item) {
            Ok(Some(args)) => {
                let content = match &mut item.content {
                    None => {
                        self.record_error(Error::new_spanned(
                            item,
                            "module to instantiate tests into must be inline",
                        ));
                        return;
                    }
                    Some((_, content)) => {
                        if !content.is_empty() {
                            self.record_error(Error::new_spanned(
                                item,
                                "module to instantiate tests into must be empty",
                            ));
                            return;
                        }
                        content
                    }
                };
                self.instantiate_tests(args, content);
            }
            Ok(None) => {
                self.depth += 1;
                visit_mut::visit_item_mod_mut(self, item);
                self.depth -= 1;
            }
            Err(e) => self.record_error(e),
        }
    }
}
