#![allow(clippy::unwrap_used)]

use bytes::Bytes;
use ps_datachunk::{DataChunk, OwnedDataChunk, PsDataChunkError};
use ps_hkey::{Hash, InMemoryStore, MAX_SIZE_RAW, PsHkeyError, Store};
use ps_htree::{HtreeKey, HtreeKeyError, HtreeValue, compact_to_uuid};
use ps_uuid::{UUID, UUID_BYTES};

#[derive(thiserror::Error, Debug)]
enum NoPutStoreError {
    #[error(transparent)]
    DataChunk(#[from] PsDataChunkError),
    #[error(transparent)]
    Hkey(#[from] PsHkeyError),
    #[error("store.put was called")]
    PutCalled,
}

#[derive(Debug, Default)]
struct NoPutStore;

impl Store for NoPutStore {
    type Chunk<'c>
        = OwnedDataChunk
    where
        Self: 'c;
    type Error = NoPutStoreError;

    fn get<'a>(&'a self, _: &Hash) -> Result<Self::Chunk<'a>, Self::Error> {
        Err(NoPutStoreError::PutCalled)
    }

    fn put_encrypted<C: DataChunk>(&self, _: C) -> Result<(), Self::Error> {
        Err(NoPutStoreError::PutCalled)
    }

    fn put(&self, _: &[u8]) -> Result<ps_hkey::Hkey, Self::Error> {
        Err(NoPutStoreError::PutCalled)
    }
}

fn compact_to_uuid_legacy(bytes: &[u8]) -> UUID {
    let mut uuid = UUID::nil();

    let range = ..bytes.len().min(UUID_BYTES);
    uuid.as_mut_bytes()[range].copy_from_slice(&bytes[range]);

    let fold = &mut uuid.as_mut_bytes()[0x8..0x10];

    for (index, slice) in bytes.chunks(0x8).skip(2).enumerate() {
        fold.rotate_left(index);

        for (byte_index, &byte) in slice.iter().enumerate() {
            fold[byte_index] ^= byte;
        }
    }

    uuid.with_version(8)
}

#[test]
fn compact_to_uuid_returns_version_8_for_any_input() {
    let uuid = compact_to_uuid(b"hello world");
    assert_eq!(uuid.get_version(), Some(8));
}

#[test]
fn compact_to_uuid_is_deterministic() {
    let input = b"same input across runs";
    let a = compact_to_uuid(input);
    let b = compact_to_uuid(input);
    assert_eq!(a, b);
}

#[test]
fn compact_to_uuid_changes_when_input_changes() {
    let a = compact_to_uuid(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let b = compact_to_uuid(b"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    assert_ne!(a, b);
}

#[test]
fn compact_to_uuid_handles_long_inputs() {
    let input = vec![0x5A; 256];
    let uuid = compact_to_uuid(&input);
    assert_eq!(uuid.get_version(), Some(8));
}

#[test]
fn compact_to_uuid_for_short_inputs_preserves_prefix_bytes() {
    let uuid = compact_to_uuid(&[1, 2, 3, 4]);
    assert_eq!(&uuid.as_bytes()[..4], &[1, 2, 3, 4]);
}

#[test]
fn compact_to_uuid_matches_legacy_for_hkey_input_range() {
    for len in 0..=84 {
        let input: Vec<u8> = (0..len).map(|i| ((i * 29 + 7) % 251) as u8).collect();

        assert_eq!(
            compact_to_uuid(&input),
            compact_to_uuid_legacy(&input),
            "mismatch at length {len}",
        );
    }
}

#[test]
fn uuid_key_round_trip_through_hkey_and_uuid() {
    let store = InMemoryStore::default();
    let key = UUID::gen_v4().with_version(8);

    let hkey = key.try_to_hkey(&store).unwrap();
    let back = hkey.try_to_uuid(&store).unwrap();

    assert_eq!(key, back);
}

#[test]
fn bytes_key_round_trip_through_uuid_is_deterministic() {
    let store = InMemoryStore::default();
    let bytes: &[u8] = b"my-key-material";

    let first = bytes.try_to_uuid(&store).unwrap();
    let second = bytes.try_to_uuid(&store).unwrap();
    assert_eq!(first, second);
}

#[test]
fn byte_container_keys_map_to_the_same_uuid() {
    let store = InMemoryStore::default();
    let array = *b"same-key-material";
    let vec = array.to_vec();
    let bytes = Bytes::copy_from_slice(&array);

    let from_array = array.try_to_uuid(&store).unwrap();
    let from_slice = array.as_slice().try_to_uuid(&store).unwrap();
    let from_vec = vec.try_to_uuid(&store).unwrap();
    let from_bytes = bytes.try_to_uuid(&store).unwrap();

    assert_eq!(from_array, from_slice);
    assert_eq!(from_array, from_vec);
    assert_eq!(from_array, from_bytes);
}

#[test]
fn sixteen_byte_inputs_use_uuid_from() {
    let store = InMemoryStore::default();
    let array = *b"0123456789abcdef";

    let expected = UUID::from(array).with_version(8);
    assert_eq!(array.try_to_uuid(&store).unwrap(), expected);
    assert_eq!(array.as_slice().try_to_uuid(&store).unwrap(), expected);
    assert_eq!(array.to_vec().try_to_uuid(&store).unwrap(), expected);
    assert_eq!(
        Bytes::copy_from_slice(&array).try_to_uuid(&store).unwrap(),
        expected
    );
}

#[test]
fn short_bytes_uuid_and_hkey_paths_match() {
    let store = InMemoryStore::default();
    let bytes: &[u8] = b"0123456789abcdef";

    let from_key = bytes.try_to_uuid(&store).unwrap();
    let from_hkey = bytes.try_to_hkey(&store).unwrap().try_to_uuid(&store).unwrap();

    assert_eq!(from_key, from_hkey);
}

#[test]
fn short_bytes_uuid_and_hkey_paths_match_for_all_raw_lengths() {
    let store = InMemoryStore::default();

    for len in 0..=MAX_SIZE_RAW {
        let payload: Vec<u8> = (0..len).map(|i| ((i * 37 + 13) % 251) as u8).collect();

        let from_key = payload.as_slice().try_to_uuid(&store).unwrap();
        let from_hkey = payload
            .as_slice()
            .try_to_hkey(&store)
            .unwrap()
            .try_to_uuid(&store)
            .unwrap();

        assert_eq!(from_key, from_hkey, "mismatch at length {len}");
    }
}

#[test]
fn uuid_conversion_skips_store_put_for_raw_sized_payloads() {
    let store = NoPutStore;

    let short_array = [0xAB; MAX_SIZE_RAW];
    let expected_array = compact_to_uuid(&short_array);
    assert_eq!(short_array.try_to_uuid(&store).unwrap(), expected_array);

    let short = vec![0xAB; MAX_SIZE_RAW];
    let expected = compact_to_uuid(&short);

    assert_eq!(short.as_slice().try_to_uuid(&store).unwrap(), expected);
    assert_eq!(short.clone().try_to_uuid(&store).unwrap(), expected);
    assert_eq!(Bytes::from(short.clone()).try_to_uuid(&store).unwrap(), expected);
}

#[test]
fn uuid_conversion_uses_store_for_payloads_above_raw_limit() {
    let store = NoPutStore;

    let long_array = [0xCD; MAX_SIZE_RAW + 1];
    let err = long_array.try_to_uuid(&store).unwrap_err();
    assert!(matches!(
        err,
        HtreeKeyError::Store(NoPutStoreError::PutCalled)
    ));

    let long = vec![0xCD; MAX_SIZE_RAW + 1];

    let err = long.as_slice().try_to_uuid(&store).unwrap_err();
    assert!(matches!(
        err,
        HtreeKeyError::Store(NoPutStoreError::PutCalled)
    ));
}

#[test]
fn string_keys_map_to_the_same_uuid() {
    let store = InMemoryStore::default();
    let text = "string-key-material";
    let owned = text.to_string();

    let from_str = text.try_to_uuid(&store).unwrap();
    let from_string = owned.try_to_uuid(&store).unwrap();
    let from_ref = (&owned).try_to_uuid(&store).unwrap();

    assert_eq!(from_str, from_string);
    assert_eq!(from_string, from_ref);
}

#[test]
fn scalar_keys_are_supported_and_deterministic() {
    let store = InMemoryStore::default();

    let n1 = 42_u64.try_to_uuid(&store).unwrap();
    let n2 = 42_u64.try_to_uuid(&store).unwrap();
    let i1 = (-7_i32).try_to_uuid(&store).unwrap();
    let i2 = (-7_i32).try_to_uuid(&store).unwrap();
    let t = true.try_to_uuid(&store).unwrap();
    let f = false.try_to_uuid(&store).unwrap();
    let c1 = 'A'.try_to_uuid(&store).unwrap();
    let c2 = 'A'.try_to_uuid(&store).unwrap();
    let f32_a = 1.5_f32.try_to_uuid(&store).unwrap();
    let f32_b = 1.5_f32.try_to_uuid(&store).unwrap();
    let f64_a = (-3.25_f64).try_to_uuid(&store).unwrap();
    let f64_b = (-3.25_f64).try_to_uuid(&store).unwrap();
    let unit_a = ().try_to_uuid(&store).unwrap();
    let unit_b = ().try_to_uuid(&store).unwrap();

    assert_eq!(n1, n2);
    assert_eq!(n1, UUID::from(42_u64));
    assert_eq!(i1, i2);
    assert_eq!(i1, UUID::from(-7_i32));
    assert_ne!(t, f);
    assert_eq!(t, UUID::from(1_u8));
    assert_eq!(f, UUID::from(0_u8));
    assert_eq!(c1, c2);
    assert_eq!(c1, UUID::from(u32::from('A')));
    assert_eq!(f32_a, f32_b);
    assert_eq!(f32_a, UUID::from(1.5_f32.to_bits()));
    assert_eq!(f64_a, f64_b);
    assert_eq!(f64_a, UUID::from((-3.25_f64).to_bits()));
    assert_eq!(unit_a, unit_b);
    assert_eq!(unit_a, UUID::from(0_u8));
}

#[test]
fn reference_forwarding_works_for_nested_and_mutable_references() {
    let store = InMemoryStore::default();
    let key = String::from("reference-key");
    let first = key.try_to_uuid(&store).unwrap();

    let key_ref = &key;
    let nested = &key_ref;
    let second = nested.try_to_uuid(&store).unwrap();

    let mut key_mut = String::from("reference-key");
    let third = (&mut key_mut).try_to_uuid(&store).unwrap();

    assert_eq!(first, second);
    assert_eq!(second, third);
}

#[test]
fn integer_zero_packs_to_empty_bytes() {
    let store = InMemoryStore::default();
    let packed = 0_u64.pack_owned(&store).unwrap();
    assert!(packed.is_empty());
}

#[test]
fn integer_nonzero_pack_unpack_round_trip() {
    let store = InMemoryStore::default();
    let value = 0x0102_0304_0506_0708_u64;

    let packed = value.pack_owned(&store).unwrap();
    let unpacked = u64::unpack(&packed, &store).unwrap();

    assert_eq!(value, unpacked);
}

#[test]
fn integer_unpack_rejects_too_many_bytes() {
    let store = InMemoryStore::default();
    let over = [1_u8; 9];
    assert!(u64::unpack(&over, &store).is_err());
}

#[test]
fn bytes_value_pack_owned_is_identity() {
    let store = InMemoryStore::default();
    let input = Bytes::from_static(b"payload");
    let packed = input.pack_owned(&store).unwrap();
    assert_eq!(packed, input);
}

#[test]
fn bytes_value_unpack_from_slice_is_identity() {
    let store = InMemoryStore::default();
    let unpacked = Bytes::unpack(b"abc123", &store).unwrap();
    assert_eq!(unpacked, Bytes::from_static(b"abc123"));
}

#[test]
fn bytes_value_unpack_from_bytes_keeps_buffer() {
    let store = InMemoryStore::default();
    let input = Bytes::from_static(b"buffer");
    let unpacked = Bytes::unpack_from_bytes(input.clone(), &store).unwrap();
    assert_eq!(unpacked, input);
}

#[test]
fn unit_value_pack_is_empty() {
    let store = InMemoryStore::default();
    let packed = ().pack_owned(&store).unwrap();
    assert!(packed.is_empty());
}

#[test]
fn unit_value_unpack_always_succeeds() {
    let store = InMemoryStore::default();
    let unpacked = <()>::unpack(b"anything", &store).unwrap();
    assert_eq!(unpacked, ());
}
