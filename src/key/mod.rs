mod error;
mod variants;

pub use error::HtreeKeyError;
use ps_hkey::{Hkey, Store};
use ps_uuid::UUID;
pub use variants::*;

pub trait HtreeKey {
    /// Stores this [`HtreeKey`] and returns its [`Hkey`].
    /// # Errors
    /// Returns an [`HtreeKeyError`] if serialization or storage fails.
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>>;

    /// Interprets this [`HtreeKey`] as a [`UUID`].
    /// # Errors
    /// Returns an [`HtreeKeyError`] if serialization or storage fails.
    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        self.try_to_hkey(store)?.try_to_uuid(store)
    }
}
