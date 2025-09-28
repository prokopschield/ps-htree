use std::convert::Infallible;

use bytes::Bytes;
use ps_hkey::Store;

use crate::HtreeValue;

impl HtreeValue for Bytes {
    type PackError = Infallible;
    type UnpackError = Infallible;

    fn pack(&self, _: impl Store) -> Result<Bytes, Self::PackError> {
        Ok(self.clone())
    }

    fn unpack(bytes: Bytes, _: impl Store) -> Result<Self, Self::UnpackError> {
        Ok(bytes)
    }
}
