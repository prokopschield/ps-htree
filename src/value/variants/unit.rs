use std::convert::Infallible;

use bytes::Bytes;

use crate::HtreeValue;

impl HtreeValue for () {
    type PackError = Infallible;
    type UnpackError = Infallible;

    fn pack_owned<S>(&self, _: &S) -> Result<Bytes, Self::PackError> {
        Ok(Bytes::from_owner(Vec::new()))
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, Self::PackError>
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
