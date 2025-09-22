use ps_hkey::{Hkey, Store};
use rkyv::{
    Archive, Serialize,
    rancor::{Error, Strategy},
    ser::{Serializer, allocator::ArenaHandle, sharing::Share},
    util::AlignedVec,
};

use crate::{HtreeKey, HtreeKeyError};

pub struct HtreeRkyvKey<T>(pub T);

impl<T> HtreeKey for HtreeRkyvKey<T>
where
    T: Archive + for<'a> Serialize<Strategy<Serializer<AlignedVec, ArenaHandle<'a>, Share>, Error>>,
{
    fn try_to_hkey<S>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>>
    where
        S: Store,
    {
        rkyv::to_bytes::<Error>(&self.0)?
            .as_ref()
            .try_to_hkey(store)
    }
}
