use ps_hkey::{Hkey, Store};
use ps_uuid::UUID;

use crate::{HtreeKey, HtreeKeyError};

impl HtreeKey for str {
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        self.as_bytes().try_to_hkey(store)
    }

    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        self.as_bytes().try_to_uuid(store)
    }
}

impl HtreeKey for String {
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        self.as_str().try_to_hkey(store)
    }

    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        self.as_str().try_to_uuid(store)
    }
}
