#![allow(clippy::expect_used)]

use std::borrow::Cow;

use bytes::Bytes;
use ps_datachunk::{DataChunk, DataChunkError, OwnedDataChunk};
use ps_hkey::{Hash, HkeyError, InMemoryStore, MAX_SIZE_RAW, Store};
use ps_htree::{HtreeKey, HtreeKeyError, HtreeValue, compact_to_uuid};
use ps_uuid::{UUID, UUID_BYTES};

#[derive(thiserror::Error, Debug)]
enum NoPutStoreError {
    #[error(transparent)]
    DataChunk(#[from] DataChunkError),
    #[error(transparent)]
    Hkey(#[from] HkeyError),
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
    for len in 0_u8..=84 {
        let input: Vec<u8> = (0..len)
            .map(|i| i.wrapping_mul(29).wrapping_add(7) % 251)
            .collect();

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

    let hkey = key.try_to_hkey(&store).expect("expected success");
    let back = hkey.try_to_uuid(&store).expect("expected success");

    assert_eq!(key, back);
}

#[test]
fn bytes_key_round_trip_through_uuid_is_deterministic() {
    let store = InMemoryStore::default();
    let bytes: &[u8] = b"my-key-material";

    let first = bytes.try_to_uuid(&store).expect("expected success");
    let second = bytes.try_to_uuid(&store).expect("expected success");
    assert_eq!(first, second);
}

#[test]
fn byte_container_keys_map_to_the_same_uuid() {
    let store = InMemoryStore::default();
    let array = *b"same-key-material";
    let vec = array.to_vec();
    let bytes = Bytes::copy_from_slice(&array);

    let from_array = array.try_to_uuid(&store).expect("expected success");
    let from_slice = array
        .as_slice()
        .try_to_uuid(&store)
        .expect("expected success");
    let from_vec = vec.try_to_uuid(&store).expect("expected success");
    let from_bytes = bytes.try_to_uuid(&store).expect("expected success");

    assert_eq!(from_array, from_slice);
    assert_eq!(from_array, from_vec);
    assert_eq!(from_array, from_bytes);
}

#[test]
fn sixteen_byte_inputs_use_uuid_from() {
    let store = InMemoryStore::default();
    let array = *b"0123456789abcdef";

    let expected = UUID::from(array).with_version(8);
    assert_eq!(
        array.try_to_uuid(&store).expect("expected success"),
        expected
    );
    assert_eq!(
        array
            .as_slice()
            .try_to_uuid(&store)
            .expect("expected success"),
        expected
    );
    assert_eq!(
        array
            .to_vec()
            .try_to_uuid(&store)
            .expect("expected success"),
        expected
    );
    assert_eq!(
        Bytes::copy_from_slice(&array)
            .try_to_uuid(&store)
            .expect("expected success"),
        expected
    );
}

#[test]
fn short_bytes_uuid_and_hkey_paths_match() {
    let store = InMemoryStore::default();
    let bytes: &[u8] = b"0123456789abcdef";

    let from_direct = bytes.try_to_uuid(&store).expect("expected success");
    let from_via_hkey = bytes
        .try_to_hkey(&store)
        .expect("expected success")
        .try_to_uuid(&store)
        .expect("expected success");

    assert_eq!(from_direct, from_via_hkey);
}

#[test]
fn short_bytes_uuid_and_hkey_paths_match_for_all_raw_lengths() {
    let store = InMemoryStore::default();

    for len in 0..=MAX_SIZE_RAW {
        let mut state = 13_u8;
        let payload: Vec<u8> = (0..len)
            .map(|_| {
                let current = state;
                state = state.wrapping_add(37);
                if state >= 251 {
                    state = state.wrapping_sub(251);
                }
                current
            })
            .collect();

        let from_direct = payload
            .as_slice()
            .try_to_uuid(&store)
            .expect("expected success");
        let from_via_hkey = payload
            .as_slice()
            .try_to_hkey(&store)
            .expect("expected success")
            .try_to_uuid(&store)
            .expect("expected success");

        assert_eq!(from_direct, from_via_hkey, "mismatch at length {len}");
    }
}

#[test]
fn uuid_conversion_skips_store_put_for_raw_sized_payloads() {
    let store = NoPutStore;

    let short_array = [0xAB; MAX_SIZE_RAW];
    let expected_array = compact_to_uuid(&short_array);
    assert_eq!(
        short_array.try_to_uuid(&store).expect("expected success"),
        expected_array
    );

    let short = vec![0xAB; MAX_SIZE_RAW];
    let expected = compact_to_uuid(&short);

    assert_eq!(
        short
            .as_slice()
            .try_to_uuid(&store)
            .expect("expected success"),
        expected
    );
    assert_eq!(
        Bytes::copy_from_slice(&short)
            .try_to_uuid(&store)
            .expect("expected success"),
        expected
    );
    assert_eq!(
        short.try_to_uuid(&store).expect("expected success"),
        expected
    );
}

#[test]
fn uuid_conversion_uses_store_for_payloads_above_raw_limit() {
    let store = NoPutStore;

    let long_array = [0xCD; MAX_SIZE_RAW + 1];
    let err = long_array.try_to_uuid(&store).expect_err("expected error");
    assert!(matches!(
        err,
        HtreeKeyError::Store(NoPutStoreError::PutCalled)
    ));

    let long = vec![0xCD; MAX_SIZE_RAW + 1];

    let err = long
        .as_slice()
        .try_to_uuid(&store)
        .expect_err("expected error");
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

    let from_str = text.try_to_uuid(&store).expect("expected success");
    let from_string = owned.try_to_uuid(&store).expect("expected success");
    let from_ref = owned.try_to_uuid(&store).expect("expected success");

    assert_eq!(from_str, from_string);
    assert_eq!(from_string, from_ref);
}

#[test]
fn scalar_keys_are_supported_and_deterministic() {
    let store = InMemoryStore::default();

    let n1 = 42_u64.try_to_uuid(&store).expect("expected success");
    let n2 = 42_u64.try_to_uuid(&store).expect("expected success");
    let i1 = (-7_i32).try_to_uuid(&store).expect("expected success");
    let i2 = (-7_i32).try_to_uuid(&store).expect("expected success");
    let t = true.try_to_uuid(&store).expect("expected success");
    let f = false.try_to_uuid(&store).expect("expected success");
    let c1 = 'A'.try_to_uuid(&store).expect("expected success");
    let c2 = 'A'.try_to_uuid(&store).expect("expected success");
    let f32_a = 1.5_f32.try_to_uuid(&store).expect("expected success");
    let f32_b = 1.5_f32.try_to_uuid(&store).expect("expected success");
    let f64_a = (-3.25_f64).try_to_uuid(&store).expect("expected success");
    let f64_b = (-3.25_f64).try_to_uuid(&store).expect("expected success");
    let unit_a = ().try_to_uuid(&store).expect("expected success");
    let unit_b = ().try_to_uuid(&store).expect("expected success");

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
    let first = key.try_to_uuid(&store).expect("expected success");

    let key_ref = &key;
    let nested = &key_ref;
    let second = nested.try_to_uuid(&store).expect("expected success");

    let mut key_mut = String::from("reference-key");
    let key_mut_ref = &mut key_mut;
    let third = key_mut_ref.try_to_uuid(&store).expect("expected success");

    assert_eq!(first, second);
    assert_eq!(second, third);
}

#[test]
fn integer_zero_packs_to_empty_bytes() {
    let store = InMemoryStore::default();
    let packed = 0_u64.pack_owned(&store).expect("expected success");
    assert!(packed.is_empty());
}

#[test]
fn integer_nonzero_pack_unpack_round_trip() {
    let store = InMemoryStore::default();
    let value = 0x0102_0304_0506_0708_u64;

    let packed = value.pack_owned(&store).expect("expected success");
    let unpacked = u64::unpack(&packed, &store).expect("expected success");

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
    let packed = input.pack_owned(&store).expect("expected success");
    assert_eq!(packed, input);
}

#[test]
fn bytes_value_unpack_from_slice_is_identity() {
    let store = InMemoryStore::default();
    let unpacked = Bytes::unpack(b"abc123", &store).expect("expected success");
    assert_eq!(unpacked, Bytes::from_static(b"abc123"));
}

#[test]
fn bytes_value_unpack_from_bytes_keeps_buffer() {
    let store = InMemoryStore::default();
    let input = Bytes::from_static(b"buffer");
    let unpacked = Bytes::unpack_from_bytes(input.clone(), &store).expect("expected success");
    assert_eq!(unpacked, input);
}

#[test]
fn cow_bytes_value_round_trip_for_borrowed_and_owned() {
    let store = InMemoryStore::default();

    let borrowed: Cow<'_, [u8]> = Cow::Borrowed(b"payload");
    let borrowed_packed = borrowed.pack_owned(&store).expect("expected success");
    let borrowed_unpacked =
        Cow::<[u8]>::unpack(&borrowed_packed, &store).expect("expected success");
    assert_eq!(borrowed_unpacked, Cow::<[u8]>::Owned(b"payload".to_vec()));

    let owned: Cow<'_, [u8]> = Cow::Owned(vec![1, 2, 3, 4]);
    let owned_packed = owned.pack_owned(&store).expect("expected success");
    let owned_unpacked =
        Cow::<[u8]>::unpack_from_bytes(owned_packed, &store).expect("expected success");
    assert_eq!(owned_unpacked, Cow::<[u8]>::Owned(vec![1, 2, 3, 4]));
}

#[test]
fn cow_str_value_round_trip_for_borrowed_and_owned() {
    let store = InMemoryStore::default();

    let borrowed: Cow<'_, str> = Cow::Borrowed("hello");
    let borrowed_packed = borrowed.pack_owned(&store).expect("expected success");
    let borrowed_unpacked = Cow::<str>::unpack(&borrowed_packed, &store).expect("expected success");
    assert_eq!(borrowed_unpacked, Cow::<str>::Owned(String::from("hello")));

    let owned: Cow<'_, str> = Cow::Owned(String::from("world"));
    let owned_packed = owned.pack_owned(&store).expect("expected success");
    let owned_unpacked =
        Cow::<str>::unpack_from_bytes(owned_packed, &store).expect("expected success");
    assert_eq!(owned_unpacked, Cow::<str>::Owned(String::from("world")));
}

#[test]
fn vec_u8_value_pack_unpack_is_identity() {
    let store = InMemoryStore::default();
    let input = vec![1_u8, 2, 3, 4, 5];

    let packed = input.pack_owned(&store).expect("expected success");
    assert_eq!(packed.as_ref(), input.as_slice());

    let unpacked = Vec::<u8>::unpack(&packed, &store).expect("expected success");
    assert_eq!(unpacked, input);
}

#[test]
fn byte_array_value_round_trip_and_length_check() {
    let store = InMemoryStore::default();
    let input = [9_u8, 8, 7, 6];

    let packed = input.pack_owned(&store).expect("expected success");
    let unpacked = <[u8; 4]>::unpack(&packed, &store).expect("expected success");

    assert_eq!(unpacked, input);
    assert!(<[u8; 4]>::unpack(b"abc", &store).is_err());
}

#[test]
fn string_value_round_trip_and_utf8_validation() {
    let store = InMemoryStore::default();
    let input = String::from("hello-htree");

    let packed = input.pack_owned(&store).expect("expected success");
    let unpacked = String::unpack(&packed, &store).expect("expected success");

    assert_eq!(unpacked, input);
    assert!(String::unpack(&[0xFF], &store).is_err());
}

#[test]
fn scalar_values_bool_char_and_floats_round_trip() {
    let store = InMemoryStore::default();

    let packed_false = false.pack_owned(&store).expect("expected success");
    let packed_true = true.pack_owned(&store).expect("expected success");
    assert!(packed_false.is_empty());
    assert_eq!(packed_true.as_ref(), &[1]);
    assert!(!bool::unpack(&packed_false, &store).expect("expected success"));
    assert!(bool::unpack(&packed_true, &store).expect("expected success"));
    assert!(bool::unpack(&[2], &store).is_err());

    let ch = 'ß';
    let packed_char = ch.pack_owned(&store).expect("expected success");
    let unpacked_char = char::unpack(&packed_char, &store).expect("expected success");
    assert_eq!(unpacked_char, ch);
    assert!(char::unpack(&[0x11, 0x00, 0x00], &store).is_err());

    let f32_val = f32::from_bits(0x7FC0_1234);
    let f32_bits = f32::unpack(
        &f32_val.pack_owned(&store).expect("expected success"),
        &store,
    )
    .expect("expected success")
    .to_bits();
    assert_eq!(f32_bits, f32_val.to_bits());

    let f64_val = f64::from_bits(0x7FF8_0000_0000_1234);
    let f64_bits = f64::unpack(
        &f64_val.pack_owned(&store).expect("expected success"),
        &store,
    )
    .expect("expected success")
    .to_bits();
    assert_eq!(f64_bits, f64_val.to_bits());
}

#[test]
fn wrapper_values_box_arc_and_rc_round_trip() {
    use std::{rc::Rc, sync::Arc};

    let store = InMemoryStore::default();

    let boxed = Box::new(String::from("boxed-value"));
    let boxed_unpacked =
        Box::<String>::unpack(&boxed.pack_owned(&store).expect("expected success"), &store)
            .expect("expected success");
    assert_eq!(*boxed_unpacked, *boxed);

    let arced = Arc::new(vec![1_u8, 2, 3]);
    let arced_unpacked =
        Arc::<Vec<u8>>::unpack(&arced.pack_owned(&store).expect("expected success"), &store)
            .expect("expected success");
    assert_eq!(*arced_unpacked, *arced);

    let rced = Rc::new(42_u64);
    let rced_unpacked =
        Rc::<u64>::unpack(&rced.pack_owned(&store).expect("expected success"), &store)
            .expect("expected success");
    assert_eq!(*rced_unpacked, *rced);
}

#[test]
fn unit_value_pack_is_empty() {
    let store = InMemoryStore::default();
    let packed = ().pack_owned(&store).expect("expected success");
    assert!(packed.is_empty());
}

#[test]
fn unit_value_unpack_always_succeeds() {
    let store = InMemoryStore::default();
    <()>::unpack(b"anything", &store).expect("expected success");
    assert_eq!((), ());
}
