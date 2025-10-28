use ps_hkey::{Hkey, Store};
use serde::Serialize;

use crate::{HtreeKey, HtreeKeyError};

pub struct HtreeSerdeKey<T>(pub T)
where
    T: Serialize;

impl<T> HtreeKey for HtreeSerdeKey<T>
where
    T: Serialize,
{
    fn try_to_hkey<S>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>>
    where
        S: Store,
    {
        postcard::to_allocvec(&self.0)?
            .as_slice()
            .try_to_hkey(store)
    }
}
