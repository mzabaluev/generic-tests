use proc_macro2::Span;
use syn::{Attribute, AttributeArgs, Error, Ident, Meta, MetaList, NestedMeta, Path};

use std::collections::HashSet;

const DEFAULT_TEST_ATTRS: &[&str] = &["test", "ignore", "should_panic", "bench"];
const DEFAULT_COPIED_ATTRS: &[&str] = &["cfg"];

pub struct MacroOpts {
    inst_attrs: HashSet<Path>,
    copy_attrs: HashSet<Path>,
}

#[derive(Default)]
pub struct TestFnOpts {
    inst_attrs: Option<HashSet<Path>>,
    copy_attrs: Option<HashSet<Path>>,
}

pub fn is_test_attr(attr: &Attribute, macro_opts: &MacroOpts, fn_opts: &TestFnOpts) -> bool {
    if let Some(attrs) = &fn_opts.inst_attrs {
        attrs.contains(&attr.path)
    } else {
        macro_opts.inst_attrs.contains(&attr.path)
    }
}

pub fn is_copied_attr(attr: &Attribute, macro_opts: &MacroOpts, fn_opts: &TestFnOpts) -> bool {
    if let Some(attrs) = &fn_opts.copy_attrs {
        attrs.contains(&attr.path)
    } else {
        macro_opts.copy_attrs.contains(&attr.path)
    }
}

fn attr_names_to_set(names: &[&str]) -> HashSet<Path> {
    names
        .iter()
        .map(|&name| Ident::new(name, Span::call_site()).into())
        .collect()
}

impl Default for MacroOpts {
    fn default() -> Self {
        MacroOpts {
            inst_attrs: attr_names_to_set(DEFAULT_TEST_ATTRS),
            copy_attrs: attr_names_to_set(DEFAULT_COPIED_ATTRS),
        }
    }
}

impl MacroOpts {
    pub fn from_args(args: AttributeArgs) -> syn::Result<Self> {
        const ERROR_MSG: &str = "unexpected attribute input; \
                                use `attrs()`, `copy_attrs()`";
        if args.is_empty() {
            return Ok(MacroOpts::default());
        }
        let mut inst_attrs = None;
        let mut copy_attrs = None;
        for nested_meta in args {
            match nested_meta {
                NestedMeta::Meta(Meta::List(list)) => {
                    if list.path.is_ident("attrs") {
                        populate_from_attrs_list(list, &mut inst_attrs)?;
                    } else if list.path.is_ident("copy_attrs") {
                        populate_from_attrs_list(list, &mut copy_attrs)?;
                    } else {
                        return Err(Error::new_spanned(list, ERROR_MSG));
                    }
                }
                _ => return Err(Error::new_spanned(nested_meta, ERROR_MSG)),
            }
        }
        Ok(MacroOpts {
            inst_attrs: inst_attrs.unwrap_or_else(|| attr_names_to_set(DEFAULT_TEST_ATTRS)),
            copy_attrs: copy_attrs.unwrap_or_else(|| attr_names_to_set(DEFAULT_COPIED_ATTRS)),
        })
    }
}

impl TestFnOpts {
    pub fn apply_attr(&mut self, attr_meta: Meta) -> syn::Result<()> {
        const ERROR_MSG: &str = "unexpected attribute input; \
                use `attrs()`, `copy_attrs()`";

        let args = match attr_meta {
            Meta::Path(path) => {
                return Err(Error::new_spanned(
                    path,
                    "attribute must have arguments; use `attrs()`, `copy_attrs()`",
                ))
            }
            Meta::List(list) => list.nested,
            Meta::NameValue(nv) => return Err(Error::new_spanned(nv, ERROR_MSG)),
        };

        for nested_meta in args {
            match nested_meta {
                NestedMeta::Meta(Meta::List(list)) => {
                    if list.path.is_ident("attrs") {
                        populate_from_attrs_list(list, &mut self.inst_attrs)?;
                    } else if list.path.is_ident("copy_attrs") {
                        populate_from_attrs_list(list, &mut self.copy_attrs)?;
                    } else {
                        return Err(Error::new_spanned(list, ERROR_MSG));
                    }
                }
                _ => return Err(Error::new_spanned(nested_meta, ERROR_MSG)),
            }
        }
        Ok(())
    }
}

fn populate_from_attrs_list(
    list: MetaList,
    customized_set: &mut Option<HashSet<Path>>,
) -> syn::Result<()> {
    if customized_set.is_none() {
        *customized_set = Some(HashSet::new());
    }
    let set = customized_set.as_mut().unwrap();
    for nested_meta in list.nested {
        match nested_meta {
            NestedMeta::Meta(Meta::Path(path)) => {
                set.insert(path);
            }
            _ => {
                return Err(Error::new_spanned(
                    nested_meta,
                    "the attribute list can only contain paths",
                ))
            }
        }
    }
    Ok(())
}
