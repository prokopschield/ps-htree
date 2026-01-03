use std::convert::Infallible;

use bytes::Bytes;
use ps_hkey::Store;

use crate::{HtreeValue, HtreeValuePackError};

impl HtreeValue for () {
    type PackError = Infallible;
    type UnpackError = Infallible;

    fn pack_owned<S>(&self, _: &S) -> Result<Bytes, HtreeValuePackError<Self, S>>
    where
        S: Store,
    {
        Ok(Bytes::from_owner(Vec::new()))
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: ps_hkey::Store,
    {
        Ok(closure(&[]))
    }

    fn unpack<S>(_: Bytes, _: &S) -> Result<Self, Self::UnpackError> {
        Ok(())
    }
}
