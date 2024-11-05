#![deny(unused)]
#![warn(clippy::all)]

#[generic_tests::define(attrs(tokio::test))]
mod async_tests {
    use bytes::{Buf, Bytes};
    use tokio::io::{self, AsyncWrite, AsyncWriteExt, Error};

    use std::pin::Pin;
    use std::task::{Context, Poll};

    #[tokio::test]
    async fn async_is_instantiated<T>() -> io::Result<()> {
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn with_worker_threads<T>() -> io::Result<()> {
        Ok(())
    }

    struct TestSink {
        data: Vec<u8>,
        ready: bool,
    }

    impl TestSink {
        fn new() -> Self {
            TestSink {
                data: Vec::new(),
                ready: false,
            }
        }
    }

    impl AsyncWrite for TestSink {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context,
            buf: &[u8],
        ) -> Poll<Result<usize, Error>> {
            if self.ready {
                self.data.extend_from_slice(buf);
                Poll::Ready(Ok(buf.len()))
            } else {
                self.ready = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Result<(), Error>> {
            Poll::Ready(Ok(()))
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Result<(), Error>> {
            Poll::Ready(Ok(()))
        }
    }

    #[tokio::test]
    async fn write_buf<T: Buf>() -> io::Result<()>
    where
        T: From<&'static str>,
    {
        let mut buf = T::from("Hello, world!");
        let mut sink = TestSink::new();
        let n = sink.write_buf(&mut buf).await?;
        assert_ne!(n, 0);
        assert_eq!(sink.data[..n], b"Hello, world!"[..n]);
        Ok(())
    }

    #[instantiate_tests(<Bytes>)]
    mod inst {}
}
