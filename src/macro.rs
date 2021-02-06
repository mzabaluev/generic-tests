//! Support for generic test definitions with a procedural attribute macro.
//!
//! The `define` macro provided by this crate allows the test writer to
//! reuse code between test cases or benchmarks that use the same test protocol
//! with different types under test. As in general programming with Rust, this
//! is achieved by using generic parameters and trait bounds. Generic
//! test functions in a module processed by the `define` macro are annotated
//! with attributes consumed by the test framework, such as `test` or `bench`,
//! however the actual test cases can be instantiated in multiple submodules
//! annotated with the `instantiate_tests` attribute providing specific
//! argument types for the tests.

#![warn(clippy::all)]
#![warn(future_incompatible)]
#![warn(rust_2018_idioms)]
#![warn(missing_docs)]

mod error;
mod expand;
mod extract;
mod options;
mod signature;

use proc_macro::TokenStream;
use syn::parse_macro_input;
use syn::{AttributeArgs, ItemMod};

/// Populates a module tree with test cases parameterizing generic definitions.
///
/// This macro is used to annotate a module containing test case definitions.
/// All functions defined immediately in the module and marked with
/// a [test attribute][test-attributes] must have the same number of generic
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
///
/// # Basic example
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
/// # fn main() {}
/// ```
///
/// # Test attributes
///
/// [test-attributes]: #test-attributes
///
/// The macro checks the paths of function attributes against a customizable
/// set of attributes that annotate the functions for the test framework.
/// Only functions with at least one of the attributes found in this set
/// are selected for instantiation.
/// These attributes are replicated to the instantiated test case functions and
/// erased from the original generic definitions. By default, the
/// `test`, `bench`, `ignore`, and `should_panic` attributes get this
/// treatment. To recognize other test attributes, their paths can be
/// listed in the `attrs()` parameter of the `define` attribute. Use of the
/// `attrs()` parameter overrides the default set.
///
/// ```
/// # #[cfg(feature = "test-tokio")]
/// #[generic_tests::define(attrs(tokio::test))]
/// mod async_tests {
///     use bytes::{Buf, Bytes};
///     use tokio::io::{self, AsyncWriteExt};
///
///     #[tokio::test]
///     async fn test_write_buf<T: Buf>() -> io::Result<()>
///     where
///         T: From<&'static str>,
///     {
///         let mut buf = T::from("Hello, world!");
///         io::sink().write_buf(&mut buf).await?;
///         Ok(())
///     }
///
///     #[instantiate_tests(<Bytes>)]
///     mod test_bytes {}
/// }
/// # fn main() {}
/// ```
///
/// The `copy_attrs()` list specifies attributes that are copied to the
/// instantiated test case functions and preserved on the generic functions.
/// By default, this set consists of `cfg`, enabling consistent conditional
/// compilation.
///
/// ```
/// # struct Foo;
/// #
/// #[generic_tests::define(copy_attrs(cfg, cfg_attr))]
/// mod tests {
///     use super::Foo;
///
///     #[test]
///     #[cfg(windows)]
///     fn test_only_on_windows<T>() {
///         // ...
///     }
///
///     #[test]
///     #[cfg_attr(feature = "my-fn-enhancer", bells_and_whistles)]
///     fn test_with_optional_bells_and_whistles<T>() {
///         // ...
///     }
///
///     #[instantiate_tests(<Foo>)]
///     mod foo {}
/// }
/// # fn main() {}
/// ```
///
/// The attribute sets can be customized for an individual generic test
/// function with the `generic_test` attribute.
///
/// ```
/// # struct Foo;
/// #
/// #[generic_tests::define]
/// mod tests {
///     use super::Foo;
///
///     #[generic_test(attrs(test, cfg_attr), copy_attrs(allow))]
///     #[test]
///     #[cfg_attr(windows, ignore)]
///     #[allow(dead_code)]
///     fn works_everywhere_except_windows<T>() {
///         // ...
///     }
///
///     #[instantiate_tests(<Foo>)]
///     mod foo {}
/// }
/// # fn main() {}
/// ```
///
/// Finally, any attributes on the generic functions' parameters are always
/// copied verbatim to the instantiated functions.
///
#[proc_macro_attribute]
pub fn define(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as AttributeArgs);
    let ast = parse_macro_input!(item as ItemMod);
    expand::expand(args, ast).into()
}
