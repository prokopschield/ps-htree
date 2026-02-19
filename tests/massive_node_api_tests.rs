#![allow(clippy::unwrap_used)]

use ps_hkey::InMemoryStore;
use ps_htree::{HtreeNode, HtreeNodeFromChildrenError};
use ps_uuid::UUID;

fn root_from_pairs(pairs: &[(UUID, u64)], store: &InMemoryStore) -> HtreeNode<u64> {
    let leaves: Vec<_> = pairs
        .iter()
        .map(|(key, value)| HtreeNode::from_kvp(key, value, store).unwrap())
        .collect();

    HtreeNode::from_many_children(leaves, store)
        .unwrap()
        .into_iter()
        .next()
        .unwrap_or_default()
}

fn combine_nodes(nodes: Vec<HtreeNode<u64>>, store: &InMemoryStore) -> HtreeNode<u64> {
    match nodes.len() {
        0 => HtreeNode::default(),
        1 => nodes.into_iter().next().unwrap(),
        _ => HtreeNode::from_many_children(nodes, store)
            .unwrap()
            .into_iter()
            .next()
            .unwrap_or_default(),
    }
}

fn sorted_pairs(count: usize) -> Vec<(UUID, u64)> {
    let mut pairs: Vec<_> = (0..count).map(|idx| (UUID::gen_v4(), idx as u64)).collect();
    pairs.sort_by_key(|(key, _)| *key);
    pairs
}

fn collect_keys(tree: &HtreeNode<u64>, store: &InMemoryStore) -> Vec<UUID> {
    tree.iter_keys(store).map(|res| res.unwrap()).collect()
}

#[test]
fn default_tree_is_empty_leaf_with_height_zero() {
    let store = InMemoryStore::default();
    let tree: HtreeNode<u64> = HtreeNode::default();

    assert!(tree.is_empty());
    assert!(tree.is_leaf());
    assert_eq!(tree.height(), 0);
    assert!(tree.fetch_children(&store).unwrap().is_empty());
}

#[test]
fn from_kvp_creates_leaf_and_find_one_returns_it() {
    let store = InMemoryStore::default();
    let key = UUID::gen_v4();
    let value = 42_u64;

    let leaf = HtreeNode::from_kvp(&key, &value, &store).unwrap();
    let found = leaf.find_one(&key, &store).unwrap().unwrap();

    assert!(leaf.is_leaf());
    assert_eq!(leaf.height(), 0);
    assert_eq!(found.key, key);
    assert_eq!(found.iter_values(&store).next().unwrap().unwrap(), value);
}

#[test]
fn from_children_sorts_children_and_increments_height() {
    let store = InMemoryStore::default();
    let mut pairs = sorted_pairs(4);
    pairs.reverse();

    let children: Vec<_> = pairs
        .iter()
        .map(|(k, v)| HtreeNode::from_kvp(k, v, &store).unwrap())
        .collect();
    let parent = HtreeNode::from_children(children, &store).unwrap();
    let fetched = parent.fetch_children(&store).unwrap();

    assert_eq!(parent.height(), 1);
    assert!(!parent.is_leaf());
    assert!(fetched.windows(2).all(|pair| pair[0].key <= pair[1].key));
}

#[test]
fn from_children_rejects_mixed_child_heights() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(3);

    let leaf_a = HtreeNode::from_kvp(&pairs[0].0, &pairs[0].1, &store).unwrap();
    let leaf_b = HtreeNode::from_kvp(&pairs[1].0, &pairs[1].1, &store).unwrap();
    let leaf_c = HtreeNode::from_kvp(&pairs[2].0, &pairs[2].1, &store).unwrap();
    let internal = HtreeNode::from_children([leaf_b, leaf_c], &store).unwrap();

    let err = HtreeNode::from_children([leaf_a, internal], &store).unwrap_err();
    assert!(matches!(
        err,
        HtreeNodeFromChildrenError::ChildHeightInconsistent
    ));
}

#[test]
fn select_child_on_leaf_matches_and_misses() {
    let store = InMemoryStore::default();
    let key = UUID::gen_v4();
    let other = UUID::gen_v4();
    let leaf = HtreeNode::from_kvp(&key, &7_u64, &store).unwrap();

    assert!(leaf.select_child(&key, &store).unwrap().is_some());
    assert!(leaf.select_child(&other, &store).unwrap().is_none());
}

#[test]
fn select_child_on_internal_returns_expected_leaf() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(5);
    let root = root_from_pairs(&pairs, &store);

    let target_key = pairs[3].0;
    let selected = root.select_child(&target_key, &store).unwrap().unwrap();

    assert_eq!(selected.key, target_key);
    assert!(selected.is_leaf());
}

#[test]
fn select_child_range_on_leaf_is_inclusive() {
    let store = InMemoryStore::default();
    let key = UUID::gen_v4();
    let leaf = HtreeNode::from_kvp(&key, &100_u64, &store).unwrap();

    let inside = leaf.select_child_range(&key, &key, &store).unwrap();
    assert_eq!(inside.len(), 1);
    assert_eq!(inside[0].key, key);
}

#[test]
fn select_child_range_returns_empty_for_inverted_bounds() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(4);
    let root = root_from_pairs(&pairs, &store);

    let empty = root
        .select_child_range(&pairs[3].0, &pairs[1].0, &store)
        .unwrap();
    assert!(empty.is_empty());
}

#[test]
fn find_one_finds_existing_and_ignores_missing() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(6);
    let root = root_from_pairs(&pairs, &store);
    let missing = UUID::gen_v4();

    let found = root.find_one(&pairs[4].0, &store).unwrap().unwrap();
    let not_found = root.find_one(&missing, &store).unwrap();

    assert_eq!(found.key, pairs[4].0);
    assert!(not_found.is_none());
}

#[test]
fn find_range_returns_only_keys_within_bounds() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(8);
    let root = root_from_pairs(&pairs, &store);

    let from = pairs[2].0;
    let to = pairs[5].0;
    let found = root.find_range(&from, &to, &store).unwrap();
    let found_keys: Vec<_> = found.iter().map(|node| node.key).collect();

    let expected: Vec<_> = pairs[2..=5].iter().map(|(k, _)| *k).collect();
    assert_eq!(found_keys, expected);
}

#[test]
fn find_range_outside_data_returns_empty() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(5);
    let root = root_from_pairs(&pairs, &store);

    let nil = UUID::nil();
    let before_first = root.find_range(&nil, &nil, &store).unwrap();
    assert!(before_first.is_empty());
}

#[test]
fn iter_values_yields_values_in_key_order() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(7);
    let root = root_from_pairs(&pairs, &store);

    let values: Vec<_> = root.iter_values(&store).map(|res| res.unwrap()).collect();
    let expected: Vec<_> = pairs.iter().map(|(_, v)| *v).collect();

    assert_eq!(values, expected);
}

#[test]
fn insert_one_inserts_into_empty_tree() {
    let store = InMemoryStore::default();
    let key = UUID::gen_v4();

    let empty: HtreeNode<u64> = HtreeNode::default();
    let inserted = combine_nodes(empty.insert_one(&key, &11_u64, &store).unwrap(), &store);

    assert!(inserted.contains_key(&key, &store).unwrap());
    assert_eq!(inserted.find_one(&key, &store).unwrap().unwrap().key, key);
}

#[test]
fn insert_many_adds_all_items() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(9);

    let empty: HtreeNode<u64> = HtreeNode::default();
    let items: Vec<_> = pairs.iter().map(|(k, v)| (k, v)).collect();
    let inserted = combine_nodes(empty.insert_many(items, &store).unwrap(), &store);

    let keys = collect_keys(&inserted, &store);
    let expected: Vec<_> = pairs.iter().map(|(k, _)| *k).collect();
    assert_eq!(keys, expected);
}

#[test]
fn update_one_updates_existing_value() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(3);
    let root = root_from_pairs(&pairs, &store);

    let updated = combine_nodes(
        root.update_one(&pairs[1].0, &999_u64, &store).unwrap(),
        &store,
    );
    let value = updated
        .find_one(&pairs[1].0, &store)
        .unwrap()
        .unwrap()
        .iter_values(&store)
        .next()
        .unwrap()
        .unwrap();

    assert_eq!(value, 999);
}

#[test]
fn update_one_on_missing_key_errors() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(2);
    let root = root_from_pairs(&pairs, &store);
    let missing = UUID::gen_v4();

    assert!(root.update_one(&missing, &1_u64, &store).is_err());
}

#[test]
fn update_many_updates_multiple_items() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(5);
    let root = root_from_pairs(&pairs, &store);

    let updates = [(&pairs[0].0, &1000_u64), (&pairs[3].0, &3000_u64)];
    let updated = combine_nodes(root.update_many(updates, &store).unwrap(), &store);
    let values: Vec<_> = updated
        .iter_values(&store)
        .map(|res| res.unwrap())
        .collect();

    assert_eq!(values[0], 1000);
    assert_eq!(values[3], 3000);
}

#[test]
fn upsert_one_inserts_then_updates_same_key() {
    let store = InMemoryStore::default();
    let key = UUID::gen_v4();
    let empty: HtreeNode<u64> = HtreeNode::default();

    let inserted = combine_nodes(empty.upsert_one(&key, &5_u64, &store).unwrap(), &store);
    let updated = combine_nodes(inserted.upsert_one(&key, &6_u64, &store).unwrap(), &store);
    let value = updated.iter_values(&store).next().unwrap().unwrap();

    assert_eq!(value, 6);
}

#[test]
fn upsert_many_mixes_existing_and_new_keys() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(3);
    let root = root_from_pairs(&pairs, &store);
    let extra_key = UUID::gen_v4();

    let upserts = [
        (&pairs[0].0, &111_u64),
        (&pairs[2].0, &333_u64),
        (&extra_key, &777_u64),
    ];
    let next = combine_nodes(root.upsert_many(upserts, &store).unwrap(), &store);
    let keys = collect_keys(&next, &store);
    let values: Vec<_> = next.iter_values(&store).map(|res| res.unwrap()).collect();

    assert!(keys.contains(&extra_key));
    assert!(values.contains(&111));
    assert!(values.contains(&333));
    assert!(values.contains(&777));
}

#[test]
fn delete_many_removes_requested_keys_and_is_idempotent() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(7);
    let root = root_from_pairs(&pairs, &store);
    let remove_a = pairs[1].0;
    let remove_b = pairs[4].0;

    let once = root
        .delete_many([&remove_a, &remove_b, &remove_b], &store)
        .unwrap();
    let twice = once.delete_many([&remove_a, &remove_b], &store).unwrap();

    assert!(!twice.contains_key(&remove_a, &store).unwrap());
    assert!(!twice.contains_key(&remove_b, &store).unwrap());
    assert_eq!(collect_keys(&once, &store), collect_keys(&twice, &store));
}

#[test]
fn unpack_children_round_trip_from_internal_node() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(4);
    let root = root_from_pairs(&pairs, &store);

    let raw = root.hkey.resolve(&store).unwrap();
    let unpacked = HtreeNode::<u64>::unpack_children(&raw).unwrap();

    assert_eq!(unpacked.len(), pairs.len());
    assert!(unpacked.iter().all(|child| child.is_leaf()));
    assert!(unpacked.windows(2).all(|pair| pair[0].key <= pair[1].key));
}

#[test]
fn unpack_children_rejects_height_zero_header() {
    let err = HtreeNode::<u64>::unpack_children(&[0]).unwrap_err();
    assert_eq!(
        err.to_string(),
        "The height of an inner node cannot be zero."
    );
}

#[test]
fn unpack_children_rejects_truncated_payload() {
    let err = HtreeNode::<u64>::unpack_children(&[1, 10, 20, 30]).unwrap_err();
    assert_eq!(err.to_string(), "Unexpected end of input");
}

#[test]
fn unpack_round_trip_rebuilds_equivalent_tree() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(6);
    let root = root_from_pairs(&pairs, &store);

    let raw = root.hkey.resolve(&store).unwrap();
    let rebuilt = HtreeNode::<u64>::unpack(&raw, &store).unwrap();

    assert_eq!(collect_keys(&rebuilt, &store), collect_keys(&root, &store));
    assert_eq!(rebuilt.height(), root.height());
}

#[test]
fn wrapped_child_can_resolve_to_its_children() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(4);

    let leaf_a = HtreeNode::from_kvp(&pairs[0].0, &pairs[0].1, &store).unwrap();
    let leaf_b = HtreeNode::from_kvp(&pairs[1].0, &pairs[1].1, &store).unwrap();
    let leaf_c = HtreeNode::from_kvp(&pairs[2].0, &pairs[2].1, &store).unwrap();
    let leaf_d = HtreeNode::from_kvp(&pairs[3].0, &pairs[3].1, &store).unwrap();

    let child_left = HtreeNode::from_children([leaf_a, leaf_b], &store).unwrap();
    let child_right = HtreeNode::from_children([leaf_c, leaf_d], &store).unwrap();
    let root = HtreeNode::from_children([child_left, child_right], &store).unwrap();

    let raw = root.hkey.resolve(&store).unwrap();
    let unpacked_children = HtreeNode::<u64>::unpack_children(&raw).unwrap();
    let wrapped = unpacked_children.first().unwrap();
    let resolved = wrapped.fetch_children(&store).unwrap();

    assert_eq!(wrapped.height(), 1);
    assert_eq!(resolved.len(), 2);
    assert!(resolved.iter().all(|node| node.is_leaf()));
}

#[test]
fn from_many_children_empty_returns_empty_vec() {
    let store = InMemoryStore::default();
    let children = HtreeNode::<u64>::from_many_children(Vec::new(), &store).unwrap();
    assert!(children.is_empty());
}

#[test]
fn from_many_children_small_input_returns_single_parent() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(3);
    let leaves: Vec<_> = pairs
        .iter()
        .map(|(k, v)| HtreeNode::from_kvp(k, v, &store).unwrap())
        .collect();

    let nodes = HtreeNode::from_many_children(leaves, &store).unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].height(), 1);
}

#[test]
fn delete_many_with_empty_keys_is_noop() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(5);
    let root = root_from_pairs(&pairs, &store);

    let unchanged = root.delete_many::<UUID, _, _>([], &store).unwrap();
    assert_eq!(
        collect_keys(&unchanged, &store),
        collect_keys(&root, &store)
    );
}

#[test]
fn find_range_single_point_returns_exact_match() {
    let store = InMemoryStore::default();
    let pairs = sorted_pairs(5);
    let root = root_from_pairs(&pairs, &store);

    let key = pairs[2].0;
    let found = root.find_range(&key, &key, &store).unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].key, key);
}

#[test]
fn unpack_children_empty_bytes_returns_empty_vec() {
    let unpacked = HtreeNode::<u64>::unpack_children(&[]).unwrap();
    assert!(unpacked.is_empty());
}

#[test]
fn unpack_empty_bytes_returns_default_node() {
    let store = InMemoryStore::default();
    let unpacked = HtreeNode::<u64>::unpack(&[], &store).unwrap();

    assert!(unpacked.is_empty());
    assert!(unpacked.is_leaf());
    assert_eq!(unpacked.height(), 0);
}

#[test]
fn resolve_on_leaf_is_noop() {
    let store = InMemoryStore::default();
    let key = UUID::gen_v4();
    let leaf = HtreeNode::from_kvp(&key, &55_u64, &store).unwrap();

    leaf.resolve(&store).unwrap();
    assert!(leaf.is_leaf());
    assert_eq!(leaf.fetch_children(&store).unwrap().len(), 0);
}
