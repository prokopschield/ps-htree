use std::{rc::Rc, sync::Arc};

use bytes::Bytes;
use ps_hkey::Store;

use crate::{HtreeValue, HtreeValuePackError, HtreeValueUnpackError};

impl<T: HtreeValue> HtreeValue for Box<T> {
    type PackError = T::PackError;
    type UnpackError = T::UnpackError;

    fn pack_owned<S: Store>(&self, store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>> {
        map_pack_error(self.as_ref().pack_owned(store))
    }

    fn pack_into<F, R, S>(&self, closure: F, store: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        map_pack_error(self.as_ref().pack_into(closure, store))
    }

    fn unpack<S: Store>(bytes: &[u8], store: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        map_unpack_error(T::unpack(bytes, store)).map(Self::new)
    }

    fn unpack_from_bytes<S: Store>(
        bytes: Bytes,
        store: &S,
    ) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        map_unpack_error(T::unpack_from_bytes(bytes, store)).map(Self::new)
    }
}

impl<T: HtreeValue> HtreeValue for Arc<T> {
    type PackError = T::PackError;
    type UnpackError = T::UnpackError;

    fn pack_owned<S: Store>(&self, store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>> {
        map_pack_error(self.as_ref().pack_owned(store))
    }

    fn pack_into<F, R, S>(&self, closure: F, store: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        map_pack_error(self.as_ref().pack_into(closure, store))
    }

    fn unpack<S: Store>(bytes: &[u8], store: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        map_unpack_error(T::unpack(bytes, store)).map(Self::new)
    }

    fn unpack_from_bytes<S: Store>(
        bytes: Bytes,
        store: &S,
    ) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        map_unpack_error(T::unpack_from_bytes(bytes, store)).map(Self::new)
    }
}

impl<T: HtreeValue> HtreeValue for Rc<T> {
    type PackError = T::PackError;
    type UnpackError = T::UnpackError;

    fn pack_owned<S: Store>(&self, store: &S) -> Result<Bytes, HtreeValuePackError<Self, S>> {
        map_pack_error(self.as_ref().pack_owned(store))
    }

    fn pack_into<F, R, S>(&self, closure: F, store: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        map_pack_error(self.as_ref().pack_into(closure, store))
    }

    fn unpack<S: Store>(bytes: &[u8], store: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        map_unpack_error(T::unpack(bytes, store)).map(Self::new)
    }

    fn unpack_from_bytes<S: Store>(
        bytes: Bytes,
        store: &S,
    ) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        map_unpack_error(T::unpack_from_bytes(bytes, store)).map(Self::new)
    }
}

fn map_pack_error<T, U, R, S>(
    result: Result<R, HtreeValuePackError<T, S>>,
) -> Result<R, HtreeValuePackError<U, S>>
where
    T: HtreeValue,
    U: HtreeValue<PackError = T::PackError, UnpackError = T::UnpackError>,
    S: Store,
{
    match result {
        Ok(value) => Ok(value),
        Err(HtreeValuePackError::Pack(pack)) => Err(HtreeValuePackError::Pack(pack)),
        Err(HtreeValuePackError::Store(store)) => Err(HtreeValuePackError::Store(store)),
    }
}

fn map_unpack_error<T, U, S>(
    result: Result<T, HtreeValueUnpackError<T, S>>,
) -> Result<T, HtreeValueUnpackError<U, S>>
where
    T: HtreeValue,
    U: HtreeValue<PackError = T::PackError, UnpackError = T::UnpackError>,
    S: Store,
{
    match result {
        Ok(value) => Ok(value),
        Err(HtreeValueUnpackError::Unpack(unpack)) => Err(HtreeValueUnpackError::Unpack(unpack)),
        Err(HtreeValueUnpackError::Store(store)) => Err(HtreeValueUnpackError::Store(store)),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use ps_hkey::InMemoryStore;

    use super::*;

    #[test]
    fn box_round_trip_and_unpack_from_bytes() {
        let store = InMemoryStore::default();
        let input = Box::new(Bytes::from_static(b"boxed"));

        let packed = input.pack_owned(&store).expect("expected success");
        let unpacked = Box::<Bytes>::unpack(&packed, &store).expect("expected success");
        assert_eq!(*unpacked, *input);

        let unpacked_from_bytes =
            Box::<Bytes>::unpack_from_bytes(Bytes::from_static(b"boxed-from-bytes"), &store)
                .expect("expected success");
        assert_eq!(
            *unpacked_from_bytes,
            Bytes::from_static(b"boxed-from-bytes")
        );
    }

    #[test]
    fn arc_round_trip_and_unpack_from_bytes() {
        let store = InMemoryStore::default();
        let input = Arc::new(Bytes::from_static(b"arc"));

        let packed = input.pack_owned(&store).expect("expected success");
        let unpacked = Arc::<Bytes>::unpack(&packed, &store).expect("expected success");
        assert_eq!(*unpacked, *input);

        let unpacked_from_bytes =
            Arc::<Bytes>::unpack_from_bytes(Bytes::from_static(b"arc-from-bytes"), &store)
                .expect("expected success");
        assert_eq!(*unpacked_from_bytes, Bytes::from_static(b"arc-from-bytes"));
    }

    #[test]
    fn rc_round_trip_and_unpack_from_bytes() {
        let store = InMemoryStore::default();
        let input = Rc::new(Bytes::from_static(b"rc"));

        let packed = input.pack_owned(&store).expect("expected success");
        let unpacked = Rc::<Bytes>::unpack(&packed, &store).expect("expected success");
        assert_eq!(*unpacked, *input);

        let unpacked_from_bytes =
            Rc::<Bytes>::unpack_from_bytes(Bytes::from_static(b"rc-from-bytes"), &store)
                .expect("expected success");
        assert_eq!(*unpacked_from_bytes, Bytes::from_static(b"rc-from-bytes"));
    }
}
