mod error;
mod variants;

use bytes::Bytes;
pub use error::{HtreeValuePackError, HtreeValueUnpackError};
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
    fn pack_owned<S: Store>(&self, store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>> {
        self.pack_into(Bytes::copy_from_slice, store)
    }

    /// Packs this `HtreeValue` into a canonical byte representation, and passes this into the provided closure.
    /// # Errors
    /// Returns a `PackError` if packing fails.
    fn pack_into<F, R, S>(&self, closure: F, store: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        Ok(closure(&self.pack_owned(store)?))
    }

    /// Unpacks this `HtreeValue` from a canonical byte representation.
    ///
    /// # Errors
    ///
    /// Returns an `UnpackError` if unpacking fails.
    fn unpack<S: Store>(bytes: &[u8], store: &S) -> Result<Self, Self::UnpackError>;

    /// Unpacks this `HtreeValue` from an owned canonical byte representation.
    ///
    /// Only implement this method if your implementation benefits from ownership of the received buffer.
    ///
    /// # Errors
    ///
    /// Returns an `UnpackError` if unpacking fails.
    fn unpack_from_bytes<S: Store>(bytes: Bytes, store: &S) -> Result<Self, Self::UnpackError> {
        Self::unpack(&bytes, store)
    }
}
