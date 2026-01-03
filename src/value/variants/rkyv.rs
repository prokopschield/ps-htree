use bytes::Bytes;
use ps_hkey::Store;
use rkyv::{
    Archive, Serialize,
    api::high::HighSerializer,
    bytecheck::CheckBytes,
    de::Pool,
    deserialize,
    rancor::{Error, Strategy},
    ser::allocator::ArenaHandle,
    to_bytes,
    util::AlignedVec,
    validation::{Validator, archive::ArchiveValidator, shared::SharedValidator},
};

use crate::{HtreeValue, HtreeValuePackError};

pub struct HtreeRkyvValue<T>(pub T);

impl<T> HtreeValue for HtreeRkyvValue<T>
where
    for<'a> <T as rkyv::Archive>::Archived:
        CheckBytes<Strategy<Validator<ArchiveValidator<'a>, SharedValidator>, Error>>,
    for<'a> T: Archive + Serialize<HighSerializer<AlignedVec, ArenaHandle<'a>, Error>>,
    <T as Archive>::Archived: rkyv::Deserialize<T, Strategy<Pool, Error>>,
{
    type UnpackError = Error;
    type PackError = Error;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>>
    where
        S: Store,
    {
        Ok(Bytes::from_owner(
            to_bytes::<Error>(&self.0).map_err(HtreeValuePackError::Pack)?,
        ))
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: ps_hkey::Store,
    {
        Ok(closure(
            &to_bytes::<Error>(&self.0).map_err(HtreeValuePackError::Pack)?,
        ))
    }

    fn unpack<S>(bytes: Bytes, _store: &S) -> Result<Self, Self::UnpackError> {
        let archived = rkyv::access::<T::Archived, Error>(&bytes)?;
        let value: T = deserialize(archived)?;

        Ok(Self(value))
    }
}
