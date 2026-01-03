use std::convert::Infallible;

use crate::HtreeValue;

macro_rules! impl_htree_value_for_ints {
    ($($ty:ty),* $(,)?) => {
        $(
            impl HtreeValue for $ty {
                type PackError = Infallible;
                type UnpackError = IntegerUnpackError;

               #[inline]
                fn pack_into<F, R, S>(
                    &self,
                    closure: F,
                    _: &S,
                ) -> Result<R, crate::HtreeValuePackError<Self, S>>
                where
                    F: FnOnce(&[u8]) -> R,
                    S: ps_hkey::Store,
                {
                    // big‑endian for deterministic ordering
                    let bytes = self.to_be_bytes();

                    // get the index of the first non-zero byte
                    let Some(index) = bytes.iter().position(|&b| b != 0) else {
                        // zero is represented by the empty byte array
                        return Ok(closure(&[]));
                    };

                    Ok(closure(&bytes[index..]))
                }

                #[inline]
                fn unpack<S>(bytes: &[u8], _: &S) -> Result<Self, crate::HtreeValueUnpackError<Self, S>>
                where
                    S: ps_hkey::Store,
                {
                    let len = bytes.len();
                    let size = std::mem::size_of::<$ty>();

                    let mut arr = [0u8; std::mem::size_of::<$ty>()];

                    if len <= size {
                        arr[size - len..].copy_from_slice(&bytes);
                        Ok(<$ty>::from_be_bytes(arr))
                    } else {
                        Err(crate::HtreeValueUnpackError::Unpack(IntegerUnpackError::TooManyBytes {
                            ty: stringify!($ty),
                            len,
                            size,
                        }))
                    }
                }
            }
        )*
    };
}

impl_htree_value_for_ints! {
    u8, u16, u32, u64, u128, usize,
    i8, i16, i32, i64, i128, isize,
}

#[derive(thiserror::Error, Debug)]
pub enum IntegerUnpackError {
    #[error("Cannot unpack {len} bytes into {ty} (size {size}).")]
    TooManyBytes {
        ty: &'static str,
        len: usize,
        size: usize,
    },
}
