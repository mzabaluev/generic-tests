# Procedural macro support for generic test definitions

The procedural attribute macro provided by this crate allows the test writer to
reuse code between test cases or benchmarks that use the same test protocol
with different types under test. As in general programming with Rust, this
is achieved by using generic parameters and trait bounds. The specific test
cases are expanded in multiple submodules with type arguments provided in
another attribute.

## Features

* Instantiates tests and benchmarks for the built-in test framework.
* Supports arbitrary test function attributes provided by other crates.
* A customizable set of attributes is copied from the generic test function to
  its instantiations.
* Supports `async` tests.

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
