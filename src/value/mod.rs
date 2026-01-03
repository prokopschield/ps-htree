mod error;
mod variants;

use bytes::Bytes;
pub use error::{HtreeValueFetchError, HtreeValueStoreError};
use ps_hkey::Store;

#[cfg(feature = "rkyv")]
pub use variants::HtreeRkyvValue;

#[cfg(feature = "serde")]
pub use variants::HtreeSerdeValue;

pub trait HtreeValue
where
    Self: Sized,
{
    type PackError;
    type UnpackError;

    /// Packs this `HtreeValue` into a canonical byte representation
    /// # Errors
    /// Returns a `PackError` if packing fails.
    fn pack<S: Store>(&self, store: &S) -> Result<Bytes, Self::PackError>;

    /// Unpacks this `HtreeValue` from a canonical byte representation
    /// # Errors
    /// Returns a `UnpackError` if packing fails.
    fn unpack<S: Store>(bytes: Bytes, store: &S) -> Result<Self, Self::UnpackError>;
}
