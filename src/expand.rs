use crate::error::ErrorRecord;
use crate::extract::{InstArguments, TestFn, Tests};

use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::parse_quote;
use syn::punctuated::Punctuated;
use syn::visit_mut::{self, VisitMut};
use syn::{Error, Expr, Item, ItemMod, Path, ReturnType, Type};

use std::collections::HashSet;

pub fn expand(mut ast: ItemMod) -> TokenStream {
    match transform(&mut ast) {
        Ok(()) => ast.into_token_stream(),
        Err(e) => e.to_compile_error(),
    }
}

fn transform(ast: &mut ItemMod) -> syn::Result<()> {
    let (tests, items) = Tests::try_extract(ast)?;
    instantiate(tests, items)
}

fn instantiate(tests: Tests, items: &mut Vec<Item>) -> syn::Result<()> {
    let mut instantiator = Instantiator {
        tests,
        depth: 1,
        errors: Default::default(),
    };
    for item in items.iter_mut() {
        instantiator.visit_item_mut(item);
    }
    instantiator.errors.check()?;
    items.push(call_sigs_mod(&instantiator.tests));
    Ok(())
}

fn call_sigs_mod(tests: &Tests) -> Item {
    let mut content = Vec::<Item>::new();
    for sig in tests.input_sigs.values() {
        let ident = &sig.item.ident;
        let generics = sig.item.lifetime_generics();
        let arg_ident = sig.args.iter().map(|arg| &arg.ident);
        let arg_ty = sig.args.iter().map(|arg| &*arg.ty);
        content.push(parse_quote! {
            pub(super) struct #ident #generics {
                #(pub #arg_ident: #arg_ty),*
            }
        })
    }
    for sig in tests.return_sigs.values() {
        let ident = &sig.item.ident;
        let generics = sig.item.lifetime_generics();
        let ty = &*sig.ty;
        content.push(parse_quote! {
            pub(super) type #ident #generics = #ty;
        })
    }
    parse_quote! {
        mod _generic_tests_call_sigs {
            #![allow(non_camel_case_types)]

            #[allow(unused_imports)]
            use super::*;

            #(#content)*
        }
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
            let name = &test.name;
            let lifetime_params = &test.lifetime_params;
            let inputs = &test.inputs;
            let output = &test.output;
            let shim_mod = self.shim_mod(test, &inst_args, &root_path);
            let args_init = self.args_init(test);
            content.push(parse_quote! {
                #(#test_attrs)*
                fn #name<#lifetime_params>(#inputs) #output {
                    #shim_mod
                    let args = #args_init;
                    let mut ret = ::core::mem::MaybeUninit::uninit();
                    unsafe {
                        shim::shim(args, ret.as_mut_ptr());
                        ret.assume_init()
                    }
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

    fn shim_mod(&self, test: &TestFn, inst_args: &InstArguments, root_path: &Path) -> Item {
        let mut root_path = root_path.clone();
        root_path.segments.push(parse_quote! { super });
        let name = &test.name;
        let (args_type, fn_args, mut lifetimes): (Type, _, _) = if test.inputs.is_empty() {
            (parse_quote! { () }, Vec::new(), HashSet::new())
        } else {
            let sig = self
                .tests
                .input_sigs
                .get(&test.inputs)
                .expect("no input signature");
            let path_seg = sig.item.to_path_segment();
            let fn_args = sig
                .args
                .iter()
                .map(|arg| -> Expr {
                    let ident = &arg.ident;
                    parse_quote! { _args.#ident }
                })
                .collect();
            (
                parse_quote! { #root_path::_generic_tests_call_sigs::#path_seg },
                fn_args,
                sig.item.lifetimes.clone(),
            )
        };
        let ret_type: Type = match &test.output {
            ReturnType::Default => parse_quote! { () },
            ReturnType::Type(_, ty) => {
                let sig = self
                    .tests
                    .return_sigs
                    .get(&*ty)
                    .expect("no return signature");
                lifetimes = lifetimes.union(&sig.item.lifetimes).cloned().collect();
                let path_seg = sig.item.to_path_segment();
                parse_quote! { #root_path::_generic_tests_call_sigs::#path_seg }
            }
        };
        // The order of lifetime parameters is not important, as the call
        // site has them inferred.
        let lifetimes = lifetimes.iter();
        parse_quote! {
            mod shim {
                #[allow(unused_imports)]
                use super::super::*;
                pub(super) unsafe fn shim<#(#lifetimes),*>(
                    _args: #args_type,
                    ret: *mut #ret_type,
                ) {
                    *ret = #root_path::#name::<#inst_args>(#(#fn_args),*)
                }
            }
        }
    }

    fn args_init(&self, test: &TestFn) -> Expr {
        if test.inputs.is_empty() {
            parse_quote! { () }
        } else {
            let sig = self
                .tests
                .input_sigs
                .get(&test.inputs)
                .expect("no input signature");
            let struct_name = &sig.item.ident;
            let field_init = sig.args.iter().map(|arg| &arg.ident);
            parse_quote! {
                _generic_tests_call_sigs::#struct_name { #(#field_init),* }
            }
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
