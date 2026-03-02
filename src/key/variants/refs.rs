use bytes::Bytes;
use ps_hkey::{Hkey, Store};
use ps_uuid::UUID;

use crate::{HtreeKey, HtreeKeyError};

impl<T> HtreeKey for &T
where
    T: HtreeKey + ?Sized,
{
    fn try_to_bytes<S: Store>(&self, store: &S) -> Result<Bytes, HtreeKeyError<S>> {
        (*self).try_to_bytes(store)
    }

    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        (*self).try_to_hkey(store)
    }

    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        (*self).try_to_uuid(store)
    }
}

impl<T> HtreeKey for &mut T
where
    T: HtreeKey + ?Sized,
{
    fn try_to_bytes<S: Store>(&self, store: &S) -> Result<Bytes, HtreeKeyError<S>> {
        (**self).try_to_bytes(store)
    }

    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        (**self).try_to_hkey(store)
    }

    fn try_to_uuid<S: Store>(&self, store: &S) -> Result<UUID, HtreeKeyError<S>> {
        (**self).try_to_uuid(store)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use bytes::Bytes;
    use ps_hkey::{Hkey, InMemoryStore, Store};
    use ps_uuid::UUID;

    use crate::{HtreeKey, HtreeKeyError};

    static TO_BYTES_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TO_HKEY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TO_UUID_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TEST_GUARD: Mutex<()> = Mutex::new(());

    #[derive(Clone, Copy, Debug)]
    struct ProbeKey;

    impl HtreeKey for ProbeKey {
        fn try_to_bytes<S: Store>(&self, _: &S) -> Result<Bytes, HtreeKeyError<S>> {
            TO_BYTES_CALLS.fetch_add(1, Ordering::SeqCst);
            Ok(Bytes::from_static(b"probe-bytes"))
        }

        fn try_to_hkey<S: Store>(&self, _: &S) -> Result<Hkey, HtreeKeyError<S>> {
            TO_HKEY_CALLS.fetch_add(1, Ordering::SeqCst);
            Hkey::from_raw(b"probe-hkey").map_err(Into::into)
        }

        fn try_to_uuid<S: Store>(&self, _: &S) -> Result<UUID, HtreeKeyError<S>> {
            TO_UUID_CALLS.fetch_add(1, Ordering::SeqCst);
            Ok(UUID::from(7_u8))
        }
    }

    fn reset_counters() {
        TO_BYTES_CALLS.store(0, Ordering::SeqCst);
        TO_HKEY_CALLS.store(0, Ordering::SeqCst);
        TO_UUID_CALLS.store(0, Ordering::SeqCst);
    }

    #[test]
    fn shared_reference_chain_does_not_recurse() -> Result<(), HtreeKeyError<InMemoryStore>> {
        let _guard = TEST_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_counters();
        let store = InMemoryStore::default();
        let key = ProbeKey;

        let r1 = &key;
        let r2 = &r1;
        let r3 = &r2;
        let r4 = &r3;

        assert_eq!(r4.try_to_bytes(&store)?, Bytes::from_static(b"probe-bytes"));
        assert_eq!(r4.try_to_hkey(&store)?, Hkey::from_raw(b"probe-hkey")?);
        assert_eq!(r4.try_to_uuid(&store)?, UUID::from(7_u8));

        assert_eq!(TO_BYTES_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(TO_HKEY_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(TO_UUID_CALLS.load(Ordering::SeqCst), 1);

        Ok(())
    }

    #[test]
    fn mutable_reference_chain_does_not_recurse() -> Result<(), HtreeKeyError<InMemoryStore>> {
        let _guard = TEST_GUARD
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reset_counters();
        let store = InMemoryStore::default();
        let mut key = ProbeKey;

        let r1 = &mut key;
        let r2 = &mut *r1;
        let r3 = &mut *r2;

        assert_eq!(r3.try_to_bytes(&store)?, Bytes::from_static(b"probe-bytes"));
        assert_eq!(r3.try_to_hkey(&store)?, Hkey::from_raw(b"probe-hkey")?);
        assert_eq!(r3.try_to_uuid(&store)?, UUID::from(7_u8));

        assert_eq!(TO_BYTES_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(TO_HKEY_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(TO_UUID_CALLS.load(Ordering::SeqCst), 1);

        Ok(())
    }
}
