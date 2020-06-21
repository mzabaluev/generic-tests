use proc_macro2::TokenStream;
use quote::quote;
use quote::ToTokens;
use syn::parse::{Parse, ParseStream};
use syn::parse_quote;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit_mut::{self, VisitMut};
use syn::Token;
use syn::{Attribute, Error, FnArg, GenericArgument, Ident, Item, ItemFn, ItemMod, Pat, PatIdent};

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

enum TestAttr {
    Test,
    Bench,
}

impl TestAttr {
    fn match_attr(attr: &Attribute) -> Option<Self> {
        if attr.path.is_ident("test") {
            Some(TestAttr::Test)
        } else if attr.path.is_ident("bench") {
            Some(TestAttr::Bench)
        } else {
            None
        }
    }
}

type TestFnArgs = Punctuated<Ident, Token![,]>;

struct TestFn {
    test_attr: TestAttr,
    name: Ident,
    inputs: Punctuated<FnArg, Token![,]>,
    args: TestFnArgs,
}

impl TestFn {
    fn try_extract(item: &mut ItemFn) -> syn::Result<Option<Self>> {
        for (pos, attr) in item.attrs.iter().enumerate() {
            if let Some(test_attr) = TestAttr::match_attr(attr) {
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
                item.attrs.remove(pos);
                return Ok(Some(TestFn {
                    test_attr,
                    name: item.sig.ident.clone(),
                    inputs: item.sig.inputs.clone(),
                    args,
                }));
            }
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

struct InstArguments(Punctuated<GenericArgument, Token![,]>);

impl InstArguments {
    fn try_extract(item: &mut ItemMod) -> syn::Result<Option<Self>> {
        for (pos, attr) in item.attrs.iter().enumerate() {
            if attr.path.is_ident("instantiate_tests") {
                let args = attr.parse_args::<Self>()?;
                item.attrs.remove(pos);
                return Ok(Some(args));
            }
        }
        Ok(None)
    }
}

impl Parse for InstArguments {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let args = Punctuated::parse_terminated(input)?;
        Ok(InstArguments(args))
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

    fn instantiate_tests(&self, args: InstArguments, content: &mut Vec<Item>) {
        debug_assert!(content.is_empty());

        let mut super_prefix = TokenStream::new();
        for _ in 0..self.depth {
            super_prefix.extend(quote! {super::});
        }

        content.push(parse_quote! { use #super_prefix*; });

        for test in &self.tests {
            let attr = match test.test_attr {
                TestAttr::Test => quote! { #[test] },
                TestAttr::Bench => quote! { #[bench] },
            };
            let name = &test.name;
            let inputs = &test.inputs;
            let generic_args = &args.0;
            let fn_args = &test.args;
            content.push(parse_quote! {
                #attr
                fn #name(#inputs) {
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
