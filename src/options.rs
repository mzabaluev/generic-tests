use proc_macro2::Span;
use syn::{Attribute, AttributeArgs, Error, Ident, Meta, MetaList, NestedMeta, Path};

use std::collections::HashSet;

const DEFAULT_TEST_ATTRS: &[&str] = &["test", "ignore", "should_panic", "bench"];
const DEFAULT_COPIED_ATTRS: &[&str] = &["cfg"];

pub struct MacroOpts {
    inst_set: HashSet<Path>,
    copy_set: HashSet<Path>,
}

fn attr_names_to_set(names: &[&str]) -> HashSet<Path> {
    names
        .iter()
        .map(|&name| Ident::new(name, Span::call_site()).into())
        .collect()
}

impl Default for MacroOpts {
    fn default() -> Self {
        let inst_set = attr_names_to_set(DEFAULT_TEST_ATTRS);
        let copy_set = attr_names_to_set(DEFAULT_COPIED_ATTRS);
        MacroOpts { inst_set, copy_set }
    }
}

impl MacroOpts {
    pub fn from_args(args: AttributeArgs) -> syn::Result<Self> {
        const ERROR_MSG: &str = "unexpected attribute input; \
                                use `attrs()`, `copy_attrs()`";
        if args.is_empty() {
            return Ok(MacroOpts::default());
        }
        let mut custom_inst_set = None;
        let mut custom_copy_set = None;
        for nested_meta in args {
            match nested_meta {
                NestedMeta::Meta(Meta::List(list)) => {
                    if list.path.is_ident("attrs") {
                        populate_from_attrs_list(list, &mut custom_inst_set)?;
                    } else if list.path.is_ident("copy_attrs") {
                        populate_from_attrs_list(list, &mut custom_copy_set)?;
                    } else {
                        return Err(Error::new_spanned(list, ERROR_MSG));
                    }
                }
                _ => return Err(Error::new_spanned(nested_meta, ERROR_MSG)),
            }
        }
        let inst_set = custom_inst_set.unwrap_or_else(|| attr_names_to_set(DEFAULT_TEST_ATTRS));
        let copy_set = custom_copy_set.unwrap_or_else(|| attr_names_to_set(DEFAULT_COPIED_ATTRS));
        Ok(MacroOpts { inst_set, copy_set })
    }

    pub fn is_test_attr(&self, attr: &Attribute) -> bool {
        self.inst_set.contains(&attr.path)
    }

    pub fn is_copied_attr(&self, attr: &Attribute) -> bool {
        self.copy_set.contains(&attr.path)
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
