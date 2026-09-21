//! Buffered loading for encodings needing whole-input validation.

use super::{Asset, AssetLoader, DataReader, STREAM_CAPACITY};
use std::task::{Context, Poll};

type AssetDecoder<T> = dyn Fn(&[u8]) -> Result<T, String>;

/// Loader for formats whose validation requires retained source bytes.
pub struct BufferedAssetLoader<T: Asset> {
    bytes: Vec<u8>,
    decode: Box<AssetDecoder<T>>,
}

impl<T: Asset> BufferedAssetLoader<T> {
    /// Source storage grows with fallible allocation; the decoder owns its output.
    pub fn new(decode: impl Fn(&[u8]) -> Result<T, String> + 'static) -> Self {
        Self {
            bytes: Vec::new(),
            decode: Box::new(decode),
        }
    }
}

impl<T: Asset> AssetLoader for BufferedAssetLoader<T> {
    type Data = T;

    fn poll_load(
        &mut self,
        reader: &mut dyn DataReader,
        cx: &mut Context<'_>,
    ) -> Poll<Result<T, String>> {
        let mut chunk = [0; STREAM_CAPACITY];
        // A host frame performs at most 1 MiB of source collection for this loader.
        for _ in 0..16 {
            match reader.poll_read(cx, &mut chunk) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(0)) => return Poll::Ready((self.decode)(&self.bytes)),
                Poll::Ready(Ok(n)) => {
                    if n > chunk.len() {
                        return Poll::Ready(Err(
                            "Data reader returned an invalid byte count".into()
                        ));
                    }
                    if let Err(error) = self.bytes.try_reserve_exact(n) {
                        return Poll::Ready(Err(error.to_string()));
                    }
                    self.bytes.extend_from_slice(&chunk[..n]);
                }
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
