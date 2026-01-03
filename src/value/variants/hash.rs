use std::convert::Infallible;

use bytes::Bytes;
use ps_hash::HashValidationError;
use ps_hkey::Hash;

use crate::HtreeValue;

impl HtreeValue for Hash {
    type PackError = Infallible;
    type UnpackError = HashValidationError;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, Self::PackError> {
        Ok(Bytes::from_owner(self.to_string()))
    }

    fn unpack<S>(bytes: Bytes, _store: &S) -> Result<Self, Self::UnpackError> {
        Self::validate(bytes)
    }
}
