use std::convert::Infallible;

use ps_hkey::Store;

use crate::{HtreeValue, HtreeValuePackError, HtreeValueUnpackError};

impl HtreeValue for bool {
    type PackError = Infallible;
    type UnpackError = BoolUnpackError;

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        Ok(if *self { closure(&[1]) } else { closure(&[]) })
    }

    fn unpack<S: Store>(bytes: &[u8], _: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        match bytes {
            [] | [0] => Ok(false),
            [1] => Ok(true),
            [value] => Err(HtreeValueUnpackError::Unpack(
                BoolUnpackError::InvalidValue { value: *value },
            )),
            _ => Err(HtreeValueUnpackError::Unpack(
                BoolUnpackError::TooManyBytes { len: bytes.len() },
            )),
        }
    }
}

impl HtreeValue for char {
    type PackError = Infallible;
    type UnpackError = CharUnpackError;

    fn pack_into<F, R, S>(&self, closure: F, store: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        u32::from(*self)
            .pack_into(closure, store)
            .map_err(map_pack_error_u32_to_char)
    }

    fn unpack<S: Store>(bytes: &[u8], store: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        let codepoint = u32::unpack(bytes, store).map_err(map_unpack_error_u32_to_char)?;

        Self::from_u32(codepoint).ok_or(HtreeValueUnpackError::Unpack(
            CharUnpackError::InvalidCodePoint { codepoint },
        ))
    }
}

impl HtreeValue for f32 {
    type PackError = Infallible;
    type UnpackError = crate::value::variants::integers::IntegerUnpackError;

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        let bits = self.to_bits();
        let bytes = bits.to_be_bytes();

        let Some(index) = bytes.iter().position(|&b| b != 0) else {
            return Ok(closure(&[]));
        };

        Ok(closure(&bytes[index..]))
    }

    fn unpack<S: Store>(bytes: &[u8], store: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        let bits = u32::unpack(bytes, store).map_err(map_unpack_error_u32_to_f32)?;
        Ok(Self::from_bits(bits))
    }
}

impl HtreeValue for f64 {
    type PackError = Infallible;
    type UnpackError = crate::value::variants::integers::IntegerUnpackError;

    fn pack_into<F, R, S>(&self, closure: F, _: &S) -> Result<R, HtreeValuePackError<Self, S>>
    where
        F: FnOnce(&[u8]) -> R,
        S: Store,
    {
        let bits = self.to_bits();
        let bytes = bits.to_be_bytes();

        let Some(index) = bytes.iter().position(|&b| b != 0) else {
            return Ok(closure(&[]));
        };

        Ok(closure(&bytes[index..]))
    }

    fn unpack<S: Store>(bytes: &[u8], store: &S) -> Result<Self, HtreeValueUnpackError<Self, S>> {
        let bits = u64::unpack(bytes, store).map_err(map_unpack_error_u64_to_f64)?;
        Ok(Self::from_bits(bits))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum BoolUnpackError {
    #[error("Cannot unpack bool from {len} bytes.")]
    TooManyBytes { len: usize },
    #[error("Cannot unpack bool from value {value}, expected 0 or 1.")]
    InvalidValue { value: u8 },
}

#[derive(thiserror::Error, Debug)]
pub enum CharUnpackError {
    #[error(transparent)]
    Integer(#[from] crate::value::variants::integers::IntegerUnpackError),
    #[error("Cannot unpack char from invalid code point U+{codepoint:04X}.")]
    InvalidCodePoint { codepoint: u32 },
}

fn map_pack_error_u32_to_char<S: Store>(
    err: HtreeValuePackError<u32, S>,
) -> HtreeValuePackError<char, S> {
    match err {
        HtreeValuePackError::Pack(_) => unreachable!("u32 pack is infallible"),
        HtreeValuePackError::Store(store) => HtreeValuePackError::Store(store),
    }
}

fn map_unpack_error_u32_to_char<S: Store>(
    err: HtreeValueUnpackError<u32, S>,
) -> HtreeValueUnpackError<char, S> {
    match err {
        HtreeValueUnpackError::Store(store) => HtreeValueUnpackError::Store(store),
        HtreeValueUnpackError::Unpack(unpack) => {
            HtreeValueUnpackError::Unpack(CharUnpackError::Integer(unpack))
        }
    }
}

fn map_unpack_error_u32_to_f32<S: Store>(
    err: HtreeValueUnpackError<u32, S>,
) -> HtreeValueUnpackError<f32, S> {
    match err {
        HtreeValueUnpackError::Store(store) => HtreeValueUnpackError::Store(store),
        HtreeValueUnpackError::Unpack(unpack) => HtreeValueUnpackError::Unpack(unpack),
    }
}

fn map_unpack_error_u64_to_f64<S: Store>(
    err: HtreeValueUnpackError<u64, S>,
) -> HtreeValueUnpackError<f64, S> {
    match err {
        HtreeValueUnpackError::Store(store) => HtreeValueUnpackError::Store(store),
        HtreeValueUnpackError::Unpack(unpack) => HtreeValueUnpackError::Unpack(unpack),
    }
}

#[cfg(test)]
mod tests {
    use ps_hkey::InMemoryStore;

    use super::*;
    use crate::HtreeValueUnpackError;

    #[test]
    fn bool_round_trip_and_error_validation() {
        let store = InMemoryStore::default();

        let packed_false = false.pack_owned(&store).expect("expected success");
        let packed_true = true.pack_owned(&store).expect("expected success");
        assert!(packed_false.is_empty());
        assert_eq!(packed_true.as_ref(), &[1]);
        assert!(!bool::unpack(&packed_false, &store).expect("expected success"));
        assert!(bool::unpack(&packed_true, &store).expect("expected success"));

        let invalid_value = bool::unpack(&[2], &store).expect_err("expected invalid value");
        assert!(matches!(
            invalid_value,
            HtreeValueUnpackError::Unpack(BoolUnpackError::InvalidValue { value: 2 })
        ));

        let too_many = bool::unpack(&[0, 1], &store).expect_err("expected too many bytes");
        assert!(matches!(
            too_many,
            HtreeValueUnpackError::Unpack(BoolUnpackError::TooManyBytes { len: 2 })
        ));
    }

    #[test]
    fn char_round_trip_and_invalid_codepoint_error() {
        let store = InMemoryStore::default();
        let input = 'ß';

        let packed = input.pack_owned(&store).expect("expected success");
        let unpacked = char::unpack(&packed, &store).expect("expected success");
        assert_eq!(unpacked, input);

        let err = char::unpack(&[0x11, 0x00, 0x00], &store)
            .expect_err("expected invalid codepoint error");
        assert!(matches!(
            err,
            HtreeValueUnpackError::Unpack(CharUnpackError::InvalidCodePoint {
                codepoint: 0x0011_0000
            })
        ));
    }

    #[test]
    fn f32_round_trip_preserves_bits_and_zero_compacts() {
        let store = InMemoryStore::default();

        let packed_zero = 0.0_f32.pack_owned(&store).expect("expected success");
        assert!(packed_zero.is_empty());

        let input = f32::from_bits(0x7FC0_1234);
        let packed = input.pack_owned(&store).expect("expected success");
        let unpacked = f32::unpack(&packed, &store).expect("expected success");
        assert_eq!(unpacked.to_bits(), input.to_bits());
    }

    #[test]
    fn f64_round_trip_preserves_bits_and_zero_compacts() {
        let store = InMemoryStore::default();

        let packed_zero = 0.0_f64.pack_owned(&store).expect("expected success");
        assert!(packed_zero.is_empty());

        let input = f64::from_bits(0x7FF8_0000_0000_1234);
        let packed = input.pack_owned(&store).expect("expected success");
        let unpacked = f64::unpack(&packed, &store).expect("expected success");
        assert_eq!(unpacked.to_bits(), input.to_bits());
    }
}
