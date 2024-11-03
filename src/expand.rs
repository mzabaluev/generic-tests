use crate::error::ErrorRecord;
use crate::extract::{InstArguments, TestFn, Tests};
use crate::options::MacroOpts;

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::punctuated::Punctuated;
use syn::visit_mut::{self, VisitMut};
use syn::{parse_quote, Token};
use syn::{Error, Expr, Item, ItemMod, Path};

pub fn expand(opts: &MacroOpts, mut ast: ItemMod) -> TokenStream {
    match transform(opts, &mut ast) {
        Ok(()) => ast.into_token_stream(),
        Err(e) => e.to_compile_error(),
    }
}

fn transform(opts: &MacroOpts, ast: &mut ItemMod) -> syn::Result<()> {
    let (tests, items) = Tests::try_extract(opts, ast)?;
    instantiate(tests, items)
}

fn instantiate(tests: Tests, items: &mut [Item]) -> syn::Result<()> {
    let mut instantiator = Instantiator {
        tests,
        depth: 1,
        errors: Default::default(),
    };
    for item in items.iter_mut() {
        instantiator.visit_item_mut(item);
    }
    instantiator.errors.check()?;
    Ok(())
}

fn shim_mod(test: &TestFn, inst_args: &InstArguments, root_path: &Path) -> Item {
    let mod_call_sig = call_sig_mod(test, root_path);
    let name = &test.ident;
    let input_sig = &test.sig.input;
    let fn_args = input_sig
        .args
        .iter()
        .map(|arg| -> Expr {
            let ident = &arg.ident;
            parse_quote! { _args.#ident }
        })
        .collect::<Punctuated<_, Token![,]>>();
    let args_path = input_sig.item.path_segment("Args");
    let return_sig = &test.sig.output;
    let ret_path = return_sig.item.path_segment("Ret");
    // The order of lifetime parameters is not important, as the call
    // site has them inferred.
    let lifetimes = input_sig.item.lifetimes.union(&return_sig.item.lifetimes);
    let asyncness = test.asyncness;
    let call = wrap_async(
        asyncness,
        parse_quote! {
            super::#root_path::#name::<#inst_args>(#fn_args)
        },
    );
    parse_quote! {
        mod shim {
            #mod_call_sig

            #[allow(unused_imports)]
            use super::super::*;

            pub(super) #asyncness unsafe fn shim<#(#lifetimes),*>(
                _args: _generic_tests_call_sig::#args_path,
            ) -> _generic_tests_call_sig::#ret_path {
                #call
            }
        }
    }
}

fn call_sig_mod(test: &TestFn, root_path: &Path) -> Item {
    let input_sig = &test.sig.input;
    let arg_generics = input_sig.item.lifetime_generics();
    let field_ident = input_sig.args.iter().map(|arg| &arg.ident);
    let field_ty = input_sig.args.iter().map(|arg| &*arg.field_ty);
    let return_sig = &test.sig.output;
    let ret_generics = return_sig.item.lifetime_generics();
    let ret_ty = &*return_sig.ty;
    parse_quote! {
        pub(super) mod _generic_tests_call_sig {
            #[allow(unused_imports)]
            use super::super::#root_path::*;

            pub(in super::super) struct Args #arg_generics {
                #(pub #field_ident: #field_ty),*
            }

            pub(super) type Ret #ret_generics = #ret_ty;
        }
    }
}

fn wrap_async(asyncness: Option<Token![async]>, expr: Expr) -> Expr {
    if asyncness.is_none() {
        expr
    } else {
        parse_quote! { #expr.await }
    }
}

struct Instantiator {
    tests: Tests,
    depth: u32,
    errors: ErrorRecord,
}

impl Instantiator {
    fn instantiate_tests(&self, inst_args: InstArguments, content: &mut Vec<Item>) {
        debug_assert!(content.is_empty());

        let root_path = self.root_path();

        content.push(parse_quote! {
            #[allow(unused_imports)]
            use #root_path::*;
        });

        for test in &self.tests.test_fns {
            let test_attrs = &test.test_attrs;
            let name = &test.ident;
            let lifetime_params = &test.sig.lifetime_params;
            let fn_args = test.sig.input.args.iter().map(|arg| arg.to_fn_arg());
            let output = &test.output;
            let mod_shim = shim_mod(test, &inst_args, &root_path);
            let args_field_init = test.sig.input.args.iter().map(|arg| &arg.ident);
            let asyncness = test.asyncness;
            let unsafety = test.unsafety;
            let call = wrap_async(
                asyncness,
                parse_quote! {
                    shim::shim(args)
                },
            );
            content.push(parse_quote! {
                #(#test_attrs)*
                #asyncness #unsafety fn #name<#lifetime_params>(#(#fn_args),*) #output {
                    #mod_shim

                    let args = shim::_generic_tests_call_sig::Args { #(#args_field_init),* };
                    unsafe { #call }
                }
            });
        }
    }

    fn root_path(&self) -> Path {
        let mut segments = Punctuated::new();
        for _ in 0..self.depth {
            segments.push(parse_quote! { super });
        }
        Path {
            leading_colon: None,
            segments,
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
                        self.errors.add_error(Error::new_spanned(
                            item,
                            "module to instantiate tests into must be inline",
                        ));
                        return;
                    }
                    Some((_, content)) => {
                        if !content.is_empty() {
                            self.errors.add_error(Error::new_spanned(
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
            Err(e) => self.errors.add_error(e),
        }
    }
}
