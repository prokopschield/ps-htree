use bytes::Bytes;
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

use crate::HtreeValue;

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

    fn pack<S>(&self, _store: &S) -> Result<Bytes, Self::PackError> {
        Ok(Bytes::from_owner(to_bytes::<Error>(&self.0)?))
    }

    fn unpack<S>(bytes: Bytes, _store: &S) -> Result<Self, Self::UnpackError> {
        let archived = rkyv::access::<T::Archived, Error>(&bytes)?;
        let value: T = deserialize(archived)?;

        Ok(Self(value))
    }
}
