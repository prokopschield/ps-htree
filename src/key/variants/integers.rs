use ps_hkey::{Hkey, Store};
use ps_uuid::UUID;

use crate::{HtreeKey, HtreeKeyError};

macro_rules! impl_htree_key_via_uuid_from {
    ($($ty:ty),* $(,)?) => {
        $(
            impl HtreeKey for $ty {
                fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
                    UUID::from(*self).try_to_hkey(store)
                }

                fn try_to_uuid<S: Store>(&self, _: &S) -> Result<UUID, HtreeKeyError<S>> {
                    Ok(UUID::from(*self))
                }
            }
        )*
    };
}

impl_htree_key_via_uuid_from! {
    u8, u16, u32, u64, u128, usize,
    i8, i16, i32, i64, i128, isize,
}

macro_rules! impl_htree_key_for_floats_via_bits {
    ($($ty:ty),* $(,)?) => {
        $(
            impl HtreeKey for $ty {
                fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
                    UUID::from(self.to_bits()).try_to_hkey(store)
                }

                fn try_to_uuid<S: Store>(&self, _: &S) -> Result<UUID, HtreeKeyError<S>> {
                    Ok(UUID::from(self.to_bits()))
                }
            }
        )*
    };
}

impl_htree_key_for_floats_via_bits!(f32, f64);

impl HtreeKey for bool {
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        UUID::from(u8::from(*self)).try_to_hkey(store)
    }

    fn try_to_uuid<S: Store>(&self, _: &S) -> Result<UUID, HtreeKeyError<S>> {
        Ok(UUID::from(u8::from(*self)))
    }
}

impl HtreeKey for char {
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        UUID::from(u32::from(*self)).try_to_hkey(store)
    }

    fn try_to_uuid<S: Store>(&self, _: &S) -> Result<UUID, HtreeKeyError<S>> {
        Ok(UUID::from(u32::from(*self)))
    }
}

impl HtreeKey for () {
    fn try_to_hkey<S: Store>(&self, store: &S) -> Result<Hkey, HtreeKeyError<S>> {
        UUID::from(0_u8).try_to_hkey(store)
    }

    fn try_to_uuid<S: Store>(&self, _: &S) -> Result<UUID, HtreeKeyError<S>> {
        Ok(UUID::from(0_u8))
    }
}
