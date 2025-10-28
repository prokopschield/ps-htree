mod error;
mod variants;

use bytes::Bytes;
pub use error::HtreeKeyError;
use ps_hkey::{Hkey, Store};
use ps_uuid::UUID;

#[cfg(feature = "rkyv")]
pub use variants::HtreeRkyvKey;

#[cfg(feature = "serde")]
pub use variants::HtreeSerdeKey;

pub trait HtreeKey {
    /// Serializes this [`HtreeKey`] and returns its byte representation.
    /// # Errors
    /// Returns an [`HtreeKeyError`] if serialization fails.
    fn try_to_bytes<S: Store>(&self, store: &S) -> Result<Bytes, HtreeKeyError<S>> {
        Ok(self.try_to_uuid(store)?.as_bytes().to_vec().into())
    }

    /// Stores this [`HtreeKey`] and returns its [`Hkey`].
    /// # Errors
    /// Returns an [`HtreeKeyError`] if serialization or storage fails.
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        store
            .put(&self.try_to_bytes(store)?)
            .map_err(HtreeKeyError::Store)
    }

    /// Interprets this [`HtreeKey`] as a [`UUID`].
    /// # Errors
    /// Returns an [`HtreeKeyError`] if serialization or storage fails.
    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        self.try_to_hkey(store)?.try_to_uuid(store)
    }
}
