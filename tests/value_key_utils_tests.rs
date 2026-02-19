#![allow(clippy::unwrap_used)]

use bytes::Bytes;
use ps_hkey::InMemoryStore;
use ps_htree::{HtreeKey, HtreeValue, compact_to_uuid};
use ps_uuid::UUID;

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
fn compact_to_uuid_for_short_inputs_preserves_prefix_bytes() {
    let uuid = compact_to_uuid(&[1, 2, 3, 4]);
    assert_eq!(&uuid.as_bytes()[..4], &[1, 2, 3, 4]);
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
