use std::convert::Infallible;

use bytes::Bytes;
use ps_hkey::{Hkey, PsHkeyError, Store};

use crate::HtreeValue;

impl HtreeValue for Hkey {
    type PackError = Infallible;
    type UnpackError = PsHkeyError;

    fn pack(&self, _: impl Store) -> Result<Bytes, Self::PackError> {
        Ok(Bytes::from_owner(self.to_string()))
    }

    fn unpack(bytes: Bytes, _: impl Store) -> Result<Self, Self::UnpackError> {
        Self::try_parse(bytes)
    }
}
