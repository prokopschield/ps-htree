use ps_hkey::{Hash, Hkey, Store};

use crate::{HtreeKey, HtreeKeyError};

impl HtreeKey for Hash {
    fn try_to_hkey<S: Store>(&self, _: &S) -> Result<Hkey, HtreeKeyError<S>> {
        Ok(Hkey::from(self))
    }
}
