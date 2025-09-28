use std::convert::Infallible;

use bytes::Bytes;
use ps_hash::HashValidationError;
use ps_hkey::{Hash, Store};

use crate::HtreeValue;

impl HtreeValue for Hash {
    type PackError = Infallible;
    type UnpackError = HashValidationError;

    fn pack(&self, _: impl Store) -> Result<Bytes, Self::PackError> {
        Ok(Bytes::from_owner(self.to_string()))
    }

    fn unpack(bytes: Bytes, _: impl Store) -> Result<Self, Self::UnpackError> {
        Self::validate(bytes)
    }
}
