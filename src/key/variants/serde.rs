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
        let mut bytes = Vec::new();

        ciborium::into_writer(&self.0, &mut bytes)?;

        bytes.as_slice().try_to_hkey(store)
    }
}
