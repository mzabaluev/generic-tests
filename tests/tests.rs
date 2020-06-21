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
}
