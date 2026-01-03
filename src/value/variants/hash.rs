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

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, Self::PackError>
    where
        F: FnOnce(&[u8]) -> R,
        S: ps_hkey::Store,
    {
        Ok(closure(self.to_string().as_bytes()))
    }

    fn unpack<S>(bytes: Bytes, _store: &S) -> Result<Self, Self::UnpackError> {
        Self::validate(bytes)
    }
}
