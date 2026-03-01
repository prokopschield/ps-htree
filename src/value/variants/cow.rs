use std::{borrow::Cow, convert::Infallible, str::Utf8Error};

use bytes::Bytes;
use ps_hkey::Store;

use crate::{HtreeValue, HtreeValuePackError, HtreeValueUnpackError};

impl HtreeValue for Cow<'_, [u8]> {
    type PackError = Infallible;
    type UnpackError = Infallible;

    fn pack_owned<S>(&self, _store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>>
    where
        S: Store,
    {
        Ok(Bytes::from_owner(self.clone().into_owned()))
    }

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        Ok(closure(self.as_ref()))
    }

    fn unpack<S: Store>(bytes: &[u8], _: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        Ok(Cow::Owned(bytes.to_vec()))
    }
}

impl HtreeValue for Cow<'_, str> {
    type PackError = Infallible;
    type UnpackError = Utf8Error;

    fn pack_owned<S>(&self, _: &S) -> Result<Bytes, HtreeValuePackError<Self, S>>
    where
        S: Store,
    {
        Ok(Bytes::from_owner(self.to_string()))
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
            .map(|value| Cow::Owned(value.to_owned()))
            .map_err(HtreeValueUnpackError::Unpack)
    }
}

#[cfg(test)]
mod tests {
    use ps_hkey::InMemoryStore;

    use super::*;

    #[test]
    fn cow_bytes_round_trip_for_borrowed_and_owned() {
        let store = InMemoryStore::default();

        let borrowed: Cow<'_, [u8]> = Cow::Borrowed(b"payload");
        let borrowed_packed = borrowed.pack_owned(&store).expect("expected success");
        let borrowed_unpacked =
            Cow::<[u8]>::unpack_from_bytes(borrowed_packed, &store).expect("expected success");
        assert_eq!(borrowed_unpacked, Cow::<[u8]>::Owned(b"payload".to_vec()));

        let owned: Cow<'_, [u8]> = Cow::Owned(vec![1, 2, 3, 4]);
        let owned_packed = owned.pack_owned(&store).expect("expected success");
        let owned_unpacked = Cow::<[u8]>::unpack(&owned_packed, &store).expect("expected success");
        assert_eq!(owned_unpacked, Cow::<[u8]>::Owned(vec![1, 2, 3, 4]));
    }

    #[test]
    fn cow_str_round_trip_and_utf8_validation() {
        let store = InMemoryStore::default();

        let borrowed: Cow<'_, str> = Cow::Borrowed("hello");
        let packed = borrowed.pack_owned(&store).expect("expected success");
        let unpacked = Cow::<str>::unpack_from_bytes(packed, &store).expect("expected success");
        assert_eq!(unpacked, Cow::<str>::Owned(String::from("hello")));

        assert!(Cow::<str>::unpack(&[0xFF], &store).is_err());
    }
}
