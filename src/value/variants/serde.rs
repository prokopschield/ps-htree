use bytes::Bytes;
use serde::{Serialize, de::DeserializeOwned};

use crate::HtreeValue;

pub struct HtreeSerdeValue<T>(pub T)
where
    T: Serialize;

impl<T> HtreeValue for HtreeSerdeValue<T>
where
    T: Serialize + DeserializeOwned,
{
    type PackError = postcard::Error;
    type UnpackError = postcard::Error;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, Self::PackError> {
        Ok(Bytes::from_owner(postcard::to_allocvec(&self.0)?))
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, Self::PackError>
    where
        F: FnOnce(&[u8]) -> R,
        S: ps_hkey::Store,
    {
        Ok(closure(&postcard::to_allocvec(&self.0)?))
    }

    fn unpack<S>(bytes: Bytes, _store: &S) -> Result<Self, Self::UnpackError> {
        Ok(Self(postcard::from_bytes(&bytes)?))
    }
}
