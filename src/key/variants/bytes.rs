use ps_hkey::{Hkey, Store};

use crate::{HtreeKey, HtreeKeyError};

impl HtreeKey for &[u8] {
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        store.put(self).map_err(|err| HtreeKeyError::Store(err))
    }
}
