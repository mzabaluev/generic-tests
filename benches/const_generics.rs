#![allow(incomplete_features)]
#![feature(const_generics)]
#![feature(test)]
#![warn(clippy::all)]

extern crate test;

#[generic_tests::define]
mod benches {
    use std::iter;
    use test::Bencher;

    #[bench]
    fn fill_vec<const LEN: usize>(b: &mut Bencher) {
        b.iter(|| {
            let v: Vec<u8> = iter::repeat(0xA5).take(LEN).collect();
            test::black_box(v);
        })
    }

    #[instantiate_tests(<16>)]
    mod small {}

    #[instantiate_tests(<65536>)]
    mod large {}
}
