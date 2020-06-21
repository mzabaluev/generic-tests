#![warn(clippy::all)]
#![warn(future_incompatible)]
#![warn(rust_2018_idioms)]

mod expand;

use proc_macro::{Span, TokenStream};
use syn::parse_macro_input;
use syn::{Error, ItemMod};

#[proc_macro_attribute]
pub fn define(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        let err = Error::new(Span::call_site().into(), "unexpected attribute input");
        return err.to_compile_error().into();
    }
    let ast = parse_macro_input!(item as ItemMod);
    expand::expand(ast).into()
}
