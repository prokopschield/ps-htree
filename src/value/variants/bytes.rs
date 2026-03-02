use std::convert::Infallible;

use bytes::Bytes;
use ps_hkey::Store;

use crate::{HtreeValue, HtreeValuePackError, HtreeValueUnpackError};

impl HtreeValue for Bytes {
    type PackError = Infallible;
    type UnpackError = Infallible;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>>
    where
        S: Store,
    {
        Ok(self.clone())
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: ps_hkey::Store,
    {
        Ok(closure(self))
    }

    fn unpack<S: Store>(bytes: &[u8], _: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        Ok(Self::copy_from_slice(bytes))
    }

    fn unpack_from_bytes<S: Store>(
        bytes: Bytes,
        _: &S,
    ) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        Ok(bytes)
    }
}

impl HtreeValue for Vec<u8> {
    type PackError = Infallible;
    type UnpackError = Infallible;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>>
    where
        S: Store,
    {
        Ok(Bytes::from_owner(self.clone()))
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: ps_hkey::Store,
    {
        Ok(closure(self))
    }

    fn unpack<S: Store>(bytes: &[u8], _: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        Ok(bytes.to_vec())
    }
}

impl<const N: usize> HtreeValue for [u8; N] {
    type PackError = Infallible;
    type UnpackError = ByteArrayUnpackError;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>>
    where
        S: Store,
    {
        Ok(Bytes::copy_from_slice(self))
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: ps_hkey::Store,
    {
        Ok(closure(self))
    }

    fn unpack<S: Store>(bytes: &[u8], _: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        bytes.try_into().map_err(|_| {
            HtreeValueUnpackError::Unpack(ByteArrayUnpackError::WrongLength {
                expected: N,
                actual: bytes.len(),
            })
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum ByteArrayUnpackError {
    #[error("Cannot unpack byte array: expected {expected} bytes, got {actual}.")]
    WrongLength { expected: usize, actual: usize },
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use ps_hkey::InMemoryStore;

    use super::*;
    use crate::HtreeValueUnpackError;

    #[test]
    fn bytes_round_trip_and_unpack_from_bytes_are_identity() {
        let store = InMemoryStore::default();
        let input = Bytes::from_static(b"bytes");

        let packed = input.pack_owned(&store).expect("expected success");
        let unpacked = Bytes::unpack(&packed, &store).expect("expected success");
        let unpacked_from_bytes =
            Bytes::unpack_from_bytes(input.clone(), &store).expect("expected success");

        assert_eq!(packed, input);
        assert_eq!(unpacked, input);
        assert_eq!(unpacked_from_bytes, input);
    }

    #[test]
    fn vec_u8_round_trip_is_identity() {
        let store = InMemoryStore::default();
        let input = vec![1_u8, 2, 3, 4];

        let packed = input.pack_owned(&store).expect("expected success");
        let unpacked = Vec::<u8>::unpack(&packed, &store).expect("expected success");

        assert_eq!(packed.as_ref(), input.as_slice());
        assert_eq!(unpacked, input);
    }

    #[test]
    fn byte_array_round_trip_and_wrong_length_error() {
        let store = InMemoryStore::default();
        let input = [9_u8, 8, 7, 6];

        let packed = input.pack_owned(&store).expect("expected success");
        let unpacked = <[u8; 4]>::unpack(&packed, &store).expect("expected success");
        assert_eq!(unpacked, input);

        let err = <[u8; 4]>::unpack(b"abc", &store).expect_err("expected wrong-length error");
        assert!(matches!(
            err,
            HtreeValueUnpackError::Unpack(ByteArrayUnpackError::WrongLength {
                expected: 4,
                actual: 3,
            })
        ));
    }
}
