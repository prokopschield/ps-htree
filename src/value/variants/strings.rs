use std::str::Utf8Error;

use bytes::Bytes;
use ps_hkey::Store;

use crate::{HtreeValue, HtreeValuePackError, HtreeValueUnpackError};

impl HtreeValue for String {
    type PackError = std::convert::Infallible;
    type UnpackError = Utf8Error;

    fn pack_owned<S>(&self, _: &S) -> Result<Bytes, HtreeValuePackError<Self, S>>
    where
        S: Store,
    {
        Ok(Bytes::from_owner(self.clone()))
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        Ok(closure(self.as_bytes()))
    }

    fn unpack<S: Store>(bytes: &[u8], _: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        std::str::from_utf8(bytes)
            .map(ToOwned::to_owned)
            .map_err(HtreeValueUnpackError::Unpack)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use ps_hkey::InMemoryStore;

    use super::*;

    #[test]
    fn string_round_trip_and_utf8_validation() {
        let store = InMemoryStore::default();
        let input = String::from("hello-htree");

        let packed = input.pack_owned(&store).expect("expected success");
        let unpacked = String::unpack_from_bytes(packed, &store).expect("expected success");
        assert_eq!(unpacked, input);

        assert!(String::unpack(&[0xFF], &store).is_err());
    }
}
