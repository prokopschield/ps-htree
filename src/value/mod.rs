mod error;
mod variants;

use bytes::Bytes;
pub use error::{HtreeValueFetchError, HtreeValueStoreError};
use ps_hkey::Store;
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
    fn pack(&self, store: impl Store) -> Result<Bytes, Self::PackError>;

    /// Unpacks this `HtreeValue` from a canonical byte representation
    /// # Errors
    /// Returns a `UnpackError` if packing fails.
    fn unpack(bytes: Bytes, store: impl Store) -> Result<Self, Self::UnpackError>;
}
