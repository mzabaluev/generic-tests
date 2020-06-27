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

    #[allow(dead_code)]
    #[cfg(test)]
    fn not_a_test_case<T>() {
        // This should not be instantiated. If it is, it will trigger
        // the dead_code lint in the instantiation module.
    }

    #[deny(dead_code)]
    #[instantiate_tests(<()>)]
    mod inst {}
}
