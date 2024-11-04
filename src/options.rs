use proc_macro2::Span;
use syn::meta::ParseNestedMeta;
use syn::parse::{Parse, ParseBuffer};
use syn::{parenthesized, Token};
use syn::{Attribute, Error, Ident, Meta, Path};

use std::collections::HashSet;

const DEFAULT_TEST_ATTRS: &[&str] = &["test", "ignore", "should_panic", "bench"];
const DEFAULT_COPIED_ATTRS: &[&str] = &["cfg"];

pub struct MacroOpts {
    inst_attrs: HashSet<Path>,
    copy_attrs: HashSet<Path>,
}

#[derive(Default)]
pub struct ParsedMacroOpts {
    inst_attrs: Option<HashSet<Path>>,
    copy_attrs: Option<HashSet<Path>>,
}

#[derive(Default)]
pub struct TestFnOpts {
    inst_attrs: Option<HashSet<Path>>,
    copy_attrs: Option<HashSet<Path>>,
}

pub fn is_test_attr(attr: &Attribute, macro_opts: &MacroOpts, fn_opts: &TestFnOpts) -> bool {
    if let Some(attrs) = &fn_opts.inst_attrs {
        attrs.contains(attr.meta.path())
    } else {
        macro_opts.inst_attrs.contains(attr.meta.path())
    }
}

pub fn is_copied_attr(attr: &Attribute, macro_opts: &MacroOpts, fn_opts: &TestFnOpts) -> bool {
    if let Some(attrs) = &fn_opts.copy_attrs {
        attrs.contains(attr.meta.path())
    } else {
        macro_opts.copy_attrs.contains(attr.meta.path())
    }
}

fn set_from_attr_names(names: &[&str]) -> HashSet<Path> {
    names
        .iter()
        .map(|&name| Ident::new(name, Span::call_site()).into())
        .collect()
}

fn populate_from_attr_list(input: &ParseBuffer<'_>, set: &mut HashSet<Path>) -> syn::Result<()> {
    let content;
    parenthesized!(content in input);
    let paths = content.parse_terminated(Path::parse, Token![,])?;
    set.extend(paths);
    Ok(())
}

impl Default for MacroOpts {
    fn default() -> Self {
        MacroOpts {
            inst_attrs: set_from_attr_names(DEFAULT_TEST_ATTRS),
            copy_attrs: set_from_attr_names(DEFAULT_COPIED_ATTRS),
        }
    }
}

impl ParsedMacroOpts {
    pub fn parse(&mut self, meta: ParseNestedMeta) -> syn::Result<()> {
        if meta.path.is_ident("attrs") {
            populate_from_attr_list(meta.input, self.inst_attrs.get_or_insert(HashSet::new()))?;
        } else if meta.path.is_ident("copy_attrs") {
            populate_from_attr_list(meta.input, self.copy_attrs.get_or_insert(HashSet::new()))?;
        } else {
            return Err(meta.error("unsupported attribute"));
        }
        Ok(())
    }

    pub fn into_effective(self) -> MacroOpts {
        MacroOpts {
            inst_attrs: self
                .inst_attrs
                .unwrap_or_else(|| set_from_attr_names(DEFAULT_TEST_ATTRS)),
            copy_attrs: self
                .copy_attrs
                .unwrap_or_else(|| set_from_attr_names(DEFAULT_COPIED_ATTRS)),
        }
    }
}

impl TestFnOpts {
    pub fn apply_attr(&mut self, attr_meta: Meta) -> syn::Result<()> {
        const ERROR_MSG: &str = "unexpected attribute input; \
                use `attrs()`, `copy_attrs()`";

        match attr_meta {
            Meta::List(list) => {
                list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("attrs") {
                        populate_from_attr_list(
                            meta.input,
                            self.inst_attrs.get_or_insert(HashSet::new()),
                        )?;
                    } else if meta.path.is_ident("copy_attrs") {
                        populate_from_attr_list(
                            meta.input,
                            self.copy_attrs.get_or_insert(HashSet::new()),
                        )?;
                    } else {
                        return Err(meta.error(ERROR_MSG));
                    }
                    Ok(())
                })?;
            }
            Meta::Path(path) => {
                return Err(Error::new_spanned(
                    path,
                    "attribute must have arguments; use `attrs()`, `copy_attrs()`",
                ))
            }
            Meta::NameValue(nv) => return Err(Error::new_spanned(nv, ERROR_MSG)),
        };
        Ok(())
    }
}
