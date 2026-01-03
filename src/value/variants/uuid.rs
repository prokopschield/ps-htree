use std::{convert::Infallible, str::FromStr};

use bytes::Bytes;
use ps_hkey::Store;
use ps_uuid::{UUID, UuidParseError};

use crate::{HtreeValue, HtreeValuePackError, HtreeValueUnpackError};

impl HtreeValue for UUID {
    type PackError = Infallible;
    type UnpackError = UuidParseError;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>>
    where
        S: Store,
    {
        Ok(Bytes::from_owner(self.to_string()))
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: ps_hkey::Store,
    {
        Ok(closure(self.to_string().as_bytes()))
    }

    fn unpack<S: Store>(bytes: &[u8], _store: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        match std::str::from_utf8(bytes) {
            Ok(str) => FromStr::from_str(str),
            Err(err) => Err(Self::UnpackError::InvalidCharacter {
                ch: bytes[err.valid_up_to()] as char,
                idx: err.valid_up_to(),
            }),
        }
        .map_err(HtreeValueUnpackError::Unpack)
    }
}
