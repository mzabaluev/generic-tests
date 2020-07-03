use proc_macro2::Span;
use syn::{Attribute, AttributeArgs, Error, Ident, Meta, MetaList, NestedMeta, Path};

use std::collections::HashSet;

const DEFAULT_TEST_ATTRS: &[&str] = &["test", "ignore", "should_panic", "bench"];
const DEFAULT_COPIED_ATTRS: &[&str] = &["cfg"];

pub struct MacroOpts {
    inst_set: HashSet<Path>,
    copy_set: HashSet<Path>,
}

impl Default for MacroOpts {
    fn default() -> Self {
        fn attr_names_to_set(names: &[&str]) -> HashSet<Path> {
            names
                .iter()
                .map(|&name| Ident::new(name, Span::call_site()).into())
                .collect()
        }

        let inst_set = attr_names_to_set(DEFAULT_TEST_ATTRS);
        let copy_set = attr_names_to_set(DEFAULT_COPIED_ATTRS);
        MacroOpts { inst_set, copy_set }
    }
}

impl MacroOpts {
    pub fn from_args(args: AttributeArgs) -> syn::Result<Self> {
        if args.is_empty() {
            return Ok(MacroOpts::default());
        }
        let mut inst_set = HashSet::new();
        let mut copy_set = HashSet::new();
        for nested_meta in args {
            match nested_meta {
                NestedMeta::Meta(Meta::List(list)) => {
                    if list.path.is_ident("attrs") {
                        populate_from_attrs_list(list, &mut inst_set)?;
                    } else if list.path.is_ident("copy_attrs") {
                        populate_from_attrs_list(list, &mut copy_set)?;
                    } else {
                        return Err(Error::new_spanned(list, "unexpected attribute input"));
                    }
                }
                _ => {
                    return Err(Error::new_spanned(
                        nested_meta,
                        "unexpected attribute input",
                    ))
                }
            }
        }
        Ok(MacroOpts { inst_set, copy_set })
    }

    pub fn is_test_attr(&self, attr: &Attribute) -> bool {
        self.inst_set.contains(&attr.path)
    }

    pub fn is_copied_attr(&self, attr: &Attribute) -> bool {
        self.copy_set.contains(&attr.path)
    }
}

fn populate_from_attrs_list(list: MetaList, set: &mut HashSet<Path>) -> syn::Result<()> {
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
