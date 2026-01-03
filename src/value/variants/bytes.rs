use std::convert::Infallible;

use bytes::Bytes;
use ps_hkey::Store;

use crate::{HtreeValue, HtreeValuePackError};

impl HtreeValue for Bytes {
    type PackError = Infallible;
    type UnpackError = Infallible;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>>
    where
        S: Store,
    {
        Ok(self.clone())
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: ps_hkey::Store,
    {
        Ok(closure(self))
    }

    fn unpack<S: Store>(bytes: &[u8], _: &S) -> Result<Self, Self::UnpackError> {
        Ok(Self::copy_from_slice(bytes))
    }

    fn unpack_from_bytes<S>(bytes: Bytes, _: &S) -> Result<Self, Self::UnpackError> {
        Ok(bytes)
    }
}
