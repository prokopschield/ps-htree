use std::convert::Infallible;

use bytes::Bytes;
use ps_hkey::{Hkey, PsHkeyError, Store};

use crate::{HtreeValue, HtreeValuePackError, HtreeValueUnpackError};

impl HtreeValue for Hkey {
    type PackError = Infallible;
    type UnpackError = PsHkeyError;

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
        Self::try_parse(bytes).map_err(HtreeValueUnpackError::Unpack)
    }
}
