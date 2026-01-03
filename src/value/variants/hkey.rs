use std::convert::Infallible;

use bytes::Bytes;
use ps_hkey::{Hkey, PsHkeyError};

use crate::HtreeValue;

impl HtreeValue for Hkey {
    type PackError = Infallible;
    type UnpackError = PsHkeyError;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, Self::PackError> {
        Ok(Bytes::from_owner(self.to_string()))
    }

    fn unpack<S>(bytes: Bytes, _store: &S) -> Result<Self, Self::UnpackError> {
        Self::try_parse(bytes)
    }
}
