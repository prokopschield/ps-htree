mod error;
mod variants;

use bytes::Bytes;
pub use error::{HtreeValueFetchError, HtreeValueStoreError};
pub use variants::*;

pub trait HtreeValue
where
    Self: Sized,
{
    type PackError;
    type UnpackError;

    /// Packs this `HtreeValue` into a canonical byte representation
    /// # Errors
    /// Returns a `PackError` if packing fails.
    fn pack<S>(&self, store: &S) -> Result<Bytes, Self::PackError>;

    /// Unpacks this `HtreeValue` from a canonical byte representation
    /// # Errors
    /// Returns a `UnpackError` if packing fails.
    fn unpack<S>(bytes: Bytes, store: &S) -> Result<Self, Self::UnpackError>;
}
