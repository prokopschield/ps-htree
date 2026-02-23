use bytes::Bytes;
use ps_hkey::{Hkey, MAX_SIZE_RAW, Store};
use ps_uuid::UUID;

use crate::{HtreeKey, HtreeKeyError, compact_to_uuid};

impl HtreeKey for [u8] {
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        store.put(self).map_err(HtreeKeyError::Store)
    }

    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        if self.len() <= MAX_SIZE_RAW {
            return Ok(compact_to_uuid(self));
        }

        self.try_to_hkey(store)?.try_to_uuid(store)
    }
}

impl<const N: usize> HtreeKey for [u8; N] {
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        self.as_slice().try_to_hkey(store)
    }

    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        if N <= MAX_SIZE_RAW {
            return Ok(compact_to_uuid(self));
        }

        self.as_slice().try_to_uuid(store)
    }
}

impl HtreeKey for Vec<u8> {
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        self.as_slice().try_to_hkey(store)
    }

    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        self.as_slice().try_to_uuid(store)
    }
}

impl HtreeKey for Bytes {
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        self.as_ref().try_to_hkey(store)
    }

    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        self.as_ref().try_to_uuid(store)
    }
}
