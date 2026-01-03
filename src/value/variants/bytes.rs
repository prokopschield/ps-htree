use std::convert::Infallible;

use bytes::Bytes;

use crate::HtreeValue;

impl HtreeValue for Bytes {
    type PackError = Infallible;
    type UnpackError = Infallible;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, Self::PackError> {
        Ok(self.clone())
    }

    fn unpack<S>(bytes: Bytes, _store: &S) -> Result<Self, Self::UnpackError> {
        Ok(bytes)
    }
}
