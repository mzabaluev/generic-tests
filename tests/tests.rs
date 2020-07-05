#[generic_tests::define]
mod simple {
    use std::borrow::Cow;
    use std::fmt::Debug;

    #[test]
    fn equates_to_str<S: From<&'static str>>()
    where
        S: ?Sized + PartialEq<str> + Debug,
    {
        let s: S = "ab".into();
        assert_eq!(&s, "ab");
        assert_ne!(&s, "aa");
    }

    #[instantiate_tests(<String>)]
    mod string {}

    #[instantiate_tests(<Cow<'static, str>>)]
    mod cow {}
}

#[generic_tests::define]
mod fallible_protocol {
    use std::borrow::Cow;
    use std::fmt::Debug;
    use std::str::{self, Utf8Error};

    trait FromUtf8<'a>: Sized + 'a {
        fn from_utf8(bytes: &'a [u8]) -> Result<Self, Utf8Error>;
    }

    impl<'a> FromUtf8<'a> for &'a str {
        fn from_utf8(bytes: &'a [u8]) -> Result<Self, Utf8Error> {
            str::from_utf8(bytes)
        }
    }

    impl<'a> FromUtf8<'a> for String {
        fn from_utf8(bytes: &'a [u8]) -> Result<Self, Utf8Error> {
            String::from_utf8(bytes.to_vec()).map_err(|e| e.utf8_error())
        }
    }

    impl<'a> FromUtf8<'a> for Cow<'a, str> {
        fn from_utf8(bytes: &'a [u8]) -> Result<Self, Utf8Error> {
            str::from_utf8(bytes).map(Into::into)
        }
    }

    #[test]
    fn ok_from_valid_utf8<'a, T>() -> Result<(), Utf8Error>
    where
        T: FromUtf8<'a> + AsRef<str> + Debug,
    {
        let v = T::from_utf8(b"Hello, world!")?;
        assert_eq!(v.as_ref(), "Hello, world!");
        Ok(())
    }

    #[instantiate_tests(<&'static str>)]
    mod str_slice {}

    #[instantiate_tests(<String>)]
    mod string {}

    #[instantiate_tests(<Cow<'static, str>>)]
    mod cow {}
}

#[generic_tests::define]
mod nested {
    use std::fmt::{self, Display};

    #[test]
    fn print<T: Display + Default>() {
        let v = T::default();
        println!("{}", v);
    }

    #[derive(Default)]
    struct Foo;

    impl Display for Foo {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            write!(f, "Foo")
        }
    }

    mod from_prelude {
        #[instantiate_tests(<String>)]
        mod string {}
    }

    mod imported {
        use std::borrow::Cow;

        #[instantiate_tests(<Cow<'static, str>>)]
        mod cow {}
    }

    mod locally_defined {
        use std::fmt::{self, Display};

        #[derive(Default)]
        struct Bar;

        impl Display for Bar {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "Bar")
            }
        }

        #[instantiate_tests(<Bar>)]
        mod bar {}
    }

    mod aliases_root {
        use super::Foo;

        #[instantiate_tests(<Foo>)]
        mod foo {}
    }

    mod shadows_root {
        use std::fmt::{self, Display};

        #[derive(Default)]
        struct Foo;

        impl Display for Foo {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "nested Foo")
            }
        }

        #[instantiate_tests(<Foo>)]
        mod foo {}
    }
}

#[generic_tests::define]
mod modifier_attrs {
    #[test]
    #[ignore]
    fn ignore<T>() {
        panic!("tests instantiated from this function should not be run")
    }

    #[test]
    #[ignore = "tests the macro works in presence of attribute content"]
    fn ignore_with_reason<T>() {
        panic!("tests instantiated from this function should not be run")
    }

    #[ignore]
    #[test]
    fn ignore_above<T>() {
        panic!("tests instantiated from this function should not be run")
    }

    #[test]
    #[should_panic]
    fn should_panic<T>() {
        panic!("panicking as it should")
    }

    #[test]
    #[should_panic(expected = "panicking as it should")]
    fn should_panic_with_expected<T>() {
        panic!("panicking as it should")
    }

    #[test]
    #[ignore]
    #[should_panic]
    fn ignore_should_panic<T>() {
        // Does not panic, so should be ignored
    }

    #[instantiate_tests(<()>)]
    mod inst {}
}

#[generic_tests::define]
mod cfg_attr {
    #[cfg(test)]
    #[test]
    fn enabled<T>() {}

    #[cfg(feature = "no-such-feature")]
    #[test]
    fn disabled_above<T>() {
        panic!("unexpectedly enabled")
    }

    #[test]
    #[cfg(feature = "no-such-feature")]
    fn disabled_below<T>() {
        panic!("unexpectedly enabled")
    }

    // This should not be instantiated. If it is, it will trigger
    // the dead_code lint in the instantiation module.
    #[allow(dead_code)]
    #[cfg(test)]
    fn not_a_test_case<T>() {}

    #[deny(dead_code)]
    #[instantiate_tests(<()>)]
    mod inst {}
}

// To test how custom attributes work, abuse the system by listing `allow`
// as a test attribute.
// A function annotated with `allow` should get instantiated, and the
// `allow` attribute on the original function should get erased. This
// does not trigger the dead code lint, though, because the instantiated
// "test" function calls the generic one and itself sports the `allow` attribute
// disabling the lint.
#[generic_tests::define(attrs(allow, test, should_panic))]
#[deny(dead_code)]
mod custom_test_attrs {

    #[allow(dead_code)]
    fn custom_sig_with_builtin_types<T>(input: i32) -> String {
        input.to_string()
    }

    struct Foo;
    struct Bar;

    #[allow(dead_code)]
    fn custom_sig_with_locally_defined_types<T>(_input: Foo) -> Bar {
        Bar
    }

    #[test]
    fn test_attr_works_too_when_listed<T>() {}

    #[test]
    #[should_panic]
    fn two_test_attrs_listed<T>() {
        panic!("panicking as it should");
    }

    #[instantiate_tests(<()>)]
    mod inst {}
}

#[generic_tests::define(copy_attrs(doc, cfg_attr))]
mod custom_copy_attrs {

    /// This illustrates how doc comments can be copied
    /// onto the instantiated tests.
    #[test]
    fn copy_doc_attrs<T>() {}

    /// This should not be instantiated. If it is, it will trigger
    /// the dead_code lint in the instantiation module.
    #[allow(dead_code)]
    fn not_a_test_case_but_has_doc<T>() {}

    #[cfg_attr(test, doc("This should not be instantiated"))]
    #[allow(dead_code)]
    fn not_a_test_case_but_has_cfg_attr<T>() {}

    #[test]
    #[cfg_attr(test, should_panic)]
    #[allow(unused_attributes)]
    fn custom_attr_gets_copied_to_instantiation<T>() {
        panic!("panicking as it should");
    }

    #[instantiate_tests(<()>)]
    #[deny(dead_code)]
    mod inst {}
}

#[generic_tests::define(attrs(allow))]
#[deny(dead_code)]
mod lifetimes_in_signature {

    struct Borrowed<'a> {
        #[allow(dead_code)]
        a: &'a str,
    }

    #[allow(dead_code)]
    fn elided_in_input<T>(_s: &str) {}

    #[allow(dead_code)]
    fn explicit_in_input<'a, T>(_s: &'a str) {}

    #[allow(dead_code)]
    fn elided_and_explicit_type_param_in_input<'b, T>(_s: &str, _b: Borrowed<'b>) {}

    #[allow(dead_code)]
    fn two_args_sharing_a_lifetime<'b, T>(s: &'b str, mut b: Borrowed<'b>) {
        b.a = s;
    }

    #[allow(dead_code)]
    fn two_args_sharing_a_lifetime_one_also_elides<'b, T>(s: &'b str, b: &mut Borrowed<'b>) {
        b.a = s;
    }

    #[allow(dead_code)]
    fn explicit_in_input_placeholder_in_output<'a, T>(a: &'a str) -> Borrowed<'_> {
        Borrowed { a }
    }

    #[allow(dead_code)]
    fn elided_in_input_placeholder_in_output<T>(a: &str) -> Borrowed<'_> {
        Borrowed { a }
    }

    #[allow(dead_code)]
    fn one_explicit_other_elided<'a, T>(a: &'a str, _b: &str) -> Borrowed<'a> {
        Borrowed { a }
    }

    #[allow(dead_code)]
    fn two_explicit<'a, 'b, T>(_a: &'a str, b: &'b str) -> Borrowed<'b> {
        Borrowed { a: b }
    }

    #[instantiate_tests(<()>)]
    mod inst {}
}

#[generic_tests::define(attrs(allow))]
mod mut_in_signature {
    #[allow(dead_code)]
    fn mut_is_erased_in_instantiation<T>(mut a: i32) {
        a += 1;
        let _ = a;
    }

    #[deny(unused_mut)]
    #[instantiate_tests(<()>)]
    mod inst {}
}
