//! Support for generic test definitions with a procedural attribute macro.
//!
//! The `define` macro provided by this crate allows the test writer to
//! reuse code between test cases or benchmarks that use the same test protocol
//! with different types under test. As in general programming with Rust, this
//! is achieved by using generic parameters and trait bounds. The generic
//! test functions are annotated with the familiar `test` or `bench` attributes,
//! however the actual test cases can be instantiated in multiple submodules
//! annotated with the `instantiate_tests` attribute providing specific
//! arguments for the tests.

#![warn(clippy::all)]
#![warn(future_incompatible)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]

mod error;
mod expand;
mod extract;
mod signature;

use proc_macro::{Span, TokenStream};
use syn::parse_macro_input;
use syn::{Error, ItemMod};

/// Populates a module tree with test cases parameterizing generic definitions.
///
/// This macro is used to annotate a module containing test case definitions.
/// All functions defined immediately in the module and marked with
/// the `test` or `bench` attribute must have the same number of generic
/// type parameters. Each function's signature must be as required
/// by the test attribute that the function is marked with; thus, the functions
/// marked with `test` must have no parameters and their return type must be
/// either `()` or `Result<(), E> where E: std::error::Error`.
///
/// Empty submodules defined inline at any depth under the module on which
/// the macro is invoked can be annotated with the `instantiate_tests`
/// attribute. The macro populates these submodules with functions whose names,
/// signatures, and test attributes mirror the generic test functions at the
/// macro invocation root module, each calling its generic namesake
/// parameterized with the arguments given in `instantiate_tests`.
/// The test attributes of the original generic definitions are erased by
/// the macro, so the test framework receives only the instantiated test cases.
/// Additionally, any `cfg` attributes on the generic function items are
/// copied to the instantiated test case functions, enabling consistent
/// conditional compilation.
///
/// # Examples
///
/// ```
/// #[generic_tests::define]
/// mod tests {
///     use std::borrow::Cow;
///     use std::fmt::Display;
///
///     #[test]
///     fn print<S>()
///     where
///         S: From<&'static str> + Display,
///     {
///         let s = S::from("Hello, world!");
///         println!("{}", s);
///     }
///
///     #[instantiate_tests(<String>)]
///     mod string {}
///
///     #[instantiate_tests(<&'static str>)]
///     mod str_slice {}
///
///     #[instantiate_tests(<Cow<'static, str>>)]
///     mod cow {}
/// }
/// ```
#[proc_macro_attribute]
pub fn define(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        let err = Error::new(Span::call_site().into(), "unexpected attribute input");
        return err.to_compile_error().into();
    }
    let ast = parse_macro_input!(item as ItemMod);
    expand::expand(ast).into()
}
