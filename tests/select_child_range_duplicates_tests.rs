#![allow(clippy::expect_used)]

use ps_hkey::InMemoryStore;
use ps_htree::HtreeNode;
use ps_uuid::UUID;

/// Creates an internal node from leaves, where some leaves share the same key.
/// This simulates scenarios where duplicate keys exist in the tree structure.
fn internal_with_duplicate_keys(keys: &[UUID], store: &InMemoryStore) -> HtreeNode<u64> {
    let leaves: Vec<_> = keys
        .iter()
        .enumerate()
        .map(|(idx, key)| HtreeNode::from_kvp(key, &(idx as u64), store).expect("expected success"))
        .collect();

    HtreeNode::from_children(leaves, store).expect("expected success")
}

#[test]
fn select_child_range_returns_all_children_with_duplicate_keys() {
    let store = InMemoryStore::default();

    // Create sorted keys where 3 consecutive children have the same key
    let key1 = UUID::from_u128(1);
    let key3 = UUID::from_u128(3);
    let key5 = UUID::from_u128(5);

    // Children: [key1, key3, key3, key3, key5] - indices 0, 1, 2, 3, 4
    let internal = internal_with_duplicate_keys(&[key1, key3, key3, key3, key5], &store);

    // Query for range [key3, key3] should return all children with key=3
    let selected = internal
        .select_child_range(&key3, &key3, &store)
        .expect("expected success");

    // Should get indices 1, 2, 3 (all children with key=3)
    // The preceding child (index 0) may also be included for range coverage,
    // but will be filtered out since it's a leaf with key != 3
    let selected_keys: Vec<UUID> = selected.iter().map(|n| n.key).collect();

    // All selected keys should be key3
    assert!(
        selected_keys.iter().all(|k| *k == key3),
        "Expected all selected keys to be key3, got: {selected_keys:?}"
    );

    // Should have exactly 3 children with key3
    assert_eq!(
        selected_keys.len(),
        3,
        "Expected 3 children with key=3, got: {}",
        selected_keys.len()
    );
}

#[test]
fn select_child_range_spanning_across_duplicates() {
    let store = InMemoryStore::default();

    // Children: [key1, key2, key2, key3, key3, key4]
    let key1 = UUID::from_u128(1);
    let key2 = UUID::from_u128(2);
    let key3 = UUID::from_u128(3);
    let key4 = UUID::from_u128(4);

    let internal = internal_with_duplicate_keys(&[key1, key2, key2, key3, key3, key4], &store);

    // Query for range [key2, key3] should include all children with key2 and key3
    let selected = internal
        .select_child_range(&key2, &key3, &store)
        .expect("expected success");

    let selected_keys: Vec<UUID> = selected.iter().map(|n| n.key).collect();

    // Should have both key2 and key3 entries
    let key2_count = selected_keys.iter().filter(|k| **k == key2).count();
    let key3_count = selected_keys.iter().filter(|k| **k == key3).count();

    assert_eq!(key2_count, 2, "Expected 2 children with key=2");
    assert_eq!(key3_count, 2, "Expected 2 children with key=3");
}

#[test]
fn select_child_range_first_duplicate_in_sequence() {
    let store = InMemoryStore::default();

    // Children: [key1, key1, key1, key2]
    // Edge case: duplicates at the beginning
    let key1 = UUID::from_u128(1);
    let key2 = UUID::from_u128(2);

    let internal = internal_with_duplicate_keys(&[key1, key1, key1, key2], &store);

    // Query for range [key1, key1]
    let selected = internal
        .select_child_range(&key1, &key1, &store)
        .expect("expected success");

    let selected_keys: Vec<UUID> = selected.iter().map(|n| n.key).collect();

    // Should get all 3 children with key1
    assert_eq!(
        selected_keys.len(),
        3,
        "Expected 3 children with key=1, got: {}",
        selected_keys.len()
    );
    assert!(
        selected_keys.iter().all(|k| *k == key1),
        "Expected all selected keys to be key1"
    );
}

#[test]
fn select_child_range_last_duplicate_in_sequence() {
    let store = InMemoryStore::default();

    // Children: [key1, key2, key2, key2]
    // Edge case: duplicates at the end
    let key1 = UUID::from_u128(1);
    let key2 = UUID::from_u128(2);

    let internal = internal_with_duplicate_keys(&[key1, key2, key2, key2], &store);

    // Query for range [key2, key2]
    let selected = internal
        .select_child_range(&key2, &key2, &store)
        .expect("expected success");

    let selected_keys: Vec<UUID> = selected.iter().map(|n| n.key).collect();

    // Should get all 3 children with key2
    assert_eq!(
        selected_keys.len(),
        3,
        "Expected 3 children with key=2, got: {}",
        selected_keys.len()
    );
    assert!(
        selected_keys.iter().all(|k| *k == key2),
        "Expected all selected keys to be key2"
    );
}
