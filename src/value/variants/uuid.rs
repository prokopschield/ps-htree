use std::{convert::Infallible, str::FromStr};

use bytes::Bytes;
use ps_uuid::{UUID, UuidParseError};

use crate::HtreeValue;

impl HtreeValue for UUID {
    type PackError = Infallible;
    type UnpackError = UuidParseError;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, Self::PackError> {
        Ok(Bytes::from_owner(self.to_string()))
    }

    fn unpack<S>(bytes: Bytes, _store: &S) -> Result<Self, Self::UnpackError> {
        match std::str::from_utf8(&bytes) {
            Ok(str) => FromStr::from_str(str),
            Err(err) => Err(Self::UnpackError::InvalidCharacter {
                ch: bytes[err.valid_up_to()] as char,
                idx: err.valid_up_to(),
            }),
        }
    }
}
