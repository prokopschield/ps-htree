use ps_hkey::Store;

use crate::{HtreeNode, HtreeNodeFromChildrenError, MAX_CHILDREN};

impl<T> HtreeNode<T> {
    /// Constructs parent nodes from an iterator of child nodes.
    ///
    /// Sorts children, increments height, groups into chunks of at most
    /// [`MAX_CHILDREN`], and hands off to [`Self::from_children`].
    ///
    /// Children with the same key are always kept in the same parent node
    /// to preserve key boundary invariants for tree operations.
    ///
    /// # Errors
    ///
    /// See [`Self::from_children`].
    pub fn from_many_children<I, S>(
        children: I,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeFromChildrenError<S>>
    where
        I: IntoIterator<Item = Self>,
        S: Store,
    {
        let mut children: Vec<Self> = children.into_iter().collect();

        if children.is_empty() {
            return Ok(Vec::new());
        }

        if children.len() <= MAX_CHILDREN {
            return Ok(vec![Self::from_children(children, store)?]);
        }

        children.sort();

        let mut buf = Vec::with_capacity(MAX_CHILDREN);
        let mut nodes = Vec::new();

        for child in children {
            if buf.len() <= 1 {
                buf.push(child);
                continue;
            }

            // Detect when the "key pattern" changes between buffer and incoming child.
            // LHS: did buffer start with duplicate keys? RHS: does incoming match the last?
            // Continue buffering when both true (all same key) or both false (all unique).
            // Flush when pattern changes: e.g., unique keys followed by duplicates, or vice versa.
            let buf_keys_repeat = buf[0].key == buf[1].key;
            let new_keys_repeat = buf[buf.len() - 1].key == child.key;
            if buf_keys_repeat == new_keys_repeat {
                buf.push(child);
                continue;
            }

            // edge case: remove non-unique key from group of unique keys
            let yeet = if new_keys_repeat { buf.pop() } else { None };

            let buf_len = buf.len();

            #[allow(clippy::iter_with_drain)]
            nodes.extend(combine_group(buf.drain(..), buf_len, store)?);

            if let Some(c) = yeet {
                buf.push(c);
            }

            buf.push(child);
        }

        if !buf.is_empty() {
            let buf_len = buf.len();
            nodes.extend(combine_group(buf, buf_len, store)?);
        }

        Ok(nodes)
    }
}

/// Combines children into one or more parent nodes, splitting evenly if needed.
fn combine_group<T, I, S>(
    children: I,
    num_children: usize,
    store: &S,
) -> Result<Vec<HtreeNode<T>>, HtreeNodeFromChildrenError<S>>
where
    I: IntoIterator<Item = HtreeNode<T>>,
    S: Store,
{
    if num_children <= MAX_CHILDREN {
        return Ok(vec![HtreeNode::from_children(children, store)?]);
    }

    let num_groups = num_children.div_ceil(MAX_CHILDREN);
    let group_size = num_children.div_ceil(num_groups);

    let mut children = children.into_iter();
    let mut groups = Vec::new();

    for _ in 0..num_groups {
        groups.push(HtreeNode::from_children(
            children.by_ref().take(group_size),
            store,
        )?);
    }

    Ok(groups)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::HashMap;

    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::{HtreeNode, MAX_CHILDREN};

    #[test]
    fn same_key_children_stay_in_same_node() {
        let store = InMemoryStore::default();

        // Create children where some share the same key
        // We need more than MAX_CHILDREN to trigger grouping
        let num_children = MAX_CHILDREN * 2 + 5;
        let num_unique_keys = 10;

        let mut children = Vec::with_capacity(num_children);
        let mut keys: Vec<UUID> = (0..num_unique_keys).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        // Distribute children across keys (some keys will have multiple children)
        for i in 0..num_children {
            let key = &keys[i % num_unique_keys];
            let leaf = HtreeNode::<u64>::from_kvp(key, &(i as u64), &store).unwrap();
            children.push(leaf);
        }

        let parent_nodes = HtreeNode::from_many_children(children, &store).unwrap();

        // For each parent node, collect all keys of its children
        // and verify that no key appears in multiple parent nodes
        let mut key_to_parent: HashMap<UUID, usize> = HashMap::new();

        for (parent_idx, parent) in parent_nodes.iter().enumerate() {
            let children = parent.fetch_children(&store).unwrap();
            for child in children {
                let child_key = child.key;
                match key_to_parent.entry(child_key) {
                    std::collections::hash_map::Entry::Vacant(e) => {
                        e.insert(parent_idx);
                    }
                    std::collections::hash_map::Entry::Occupied(e) => {
                        // Same key appearing multiple times is OK if it's in the same parent
                        if *e.get() != parent_idx {
                            panic!(
                                "Key {:?} found in both parent {} and parent {}",
                                child_key,
                                e.get(),
                                parent_idx
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn all_same_key_creates_multiple_nodes() {
        let store = InMemoryStore::default();

        // All children have the same key
        let key = UUID::gen_v4();
        let num_children = MAX_CHILDREN + 10;

        let children: Vec<_> = (0..num_children)
            .map(|i| HtreeNode::<u64>::from_kvp(&key, &(i as u64), &store).unwrap())
            .collect();

        let parent_nodes = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(
            parent_nodes.len(),
            2,
            "> MAX_CHILDREN keys should split across multiple nodes"
        );
    }

    #[test]
    fn unique_keys_behave_like_original() {
        let store = InMemoryStore::default();

        // Each child has a unique key
        let num_children = MAX_CHILDREN * 2 + 5;

        let children: Vec<_> = (0..num_children)
            .map(|i| {
                let key = UUID::gen_v4();
                HtreeNode::<u64>::from_kvp(&key, &(i as u64), &store).unwrap()
            })
            .collect();

        let parent_nodes = HtreeNode::from_many_children(children, &store).unwrap();

        // Should create multiple nodes
        assert!(
            parent_nodes.len() > 1,
            "Many unique-key children should create multiple parent nodes"
        );

        // Total children across all parents should equal original count
        let total_children: usize = parent_nodes
            .iter()
            .map(|p| p.fetch_children(&store).unwrap().len())
            .sum();
        assert_eq!(total_children, num_children);
    }

    #[test]
    fn preserves_all_keys() {
        let store = InMemoryStore::default();

        let num_children = MAX_CHILDREN * 3;
        let num_unique_keys = 20;

        let mut keys: Vec<UUID> = (0..num_unique_keys).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let mut expected_key_counts: HashMap<UUID, usize> = HashMap::new();
        let children: Vec<_> = (0..num_children)
            .map(|i| {
                let key = keys[i % num_unique_keys];
                *expected_key_counts.entry(key).or_insert(0) += 1;
                HtreeNode::<u64>::from_kvp(&key, &(i as u64), &store).unwrap()
            })
            .collect();

        let parent_nodes = HtreeNode::from_many_children(children, &store).unwrap();

        // Count actual keys in result
        let mut actual_key_counts: HashMap<UUID, usize> = HashMap::new();
        for parent in &parent_nodes {
            for child in parent.fetch_children(&store).unwrap() {
                *actual_key_counts.entry(child.key).or_insert(0) += 1;
            }
        }

        assert_eq!(
            expected_key_counts, actual_key_counts,
            "All keys should be preserved with correct counts"
        );
    }

    #[test]
    fn singleton_before_duplicates() {
        let store = InMemoryStore::default();

        // Construct: A(1x), B(2x), C(1x), ... to trigger the pattern transition bug
        let mut children = Vec::new();
        let mut keys: Vec<UUID> = (0..MAX_CHILDREN).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        // One child for first key (singleton)
        children.push(HtreeNode::<u64>::from_kvp(&keys[0], &0, &store).unwrap());
        // Two children for second key (duplicate run after singleton)
        children.push(HtreeNode::<u64>::from_kvp(&keys[1], &1, &store).unwrap());
        children.push(HtreeNode::<u64>::from_kvp(&keys[1], &2, &store).unwrap());
        // Fill rest with singletons to exceed MAX_CHILDREN
        for (i, key) in keys.iter().enumerate().skip(2) {
            children.push(HtreeNode::<u64>::from_kvp(key, &(i as u64), &store).unwrap());
        }

        let parent_nodes = HtreeNode::from_many_children(children, &store).unwrap();

        // Verify no key spans multiple parents
        let mut key_to_parent: HashMap<UUID, usize> = HashMap::new();
        for (idx, parent) in parent_nodes.iter().enumerate() {
            for child in parent.fetch_children(&store).unwrap() {
                if let Some(&prev_idx) = key_to_parent.get(&child.key) {
                    assert_eq!(
                        prev_idx, idx,
                        "Key {:?} split across parent {} and {}",
                        child.key, prev_idx, idx
                    );
                }
                key_to_parent.insert(child.key, idx);
            }
        }
    }

    #[test]
    fn unique_then_duplicate_stays_single_group() {
        let store = InMemoryStore::default();

        // Construct: A(1x), B(2x) -> should remain in one group when under MAX_CHILDREN.
        let mut keys: Vec<UUID> = (0..2).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let children = vec![
            HtreeNode::<u64>::from_kvp(&keys[0], &0, &store).unwrap(),
            HtreeNode::<u64>::from_kvp(&keys[1], &1, &store).unwrap(),
            HtreeNode::<u64>::from_kvp(&keys[1], &2, &store).unwrap(),
        ];

        let parent_nodes = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parent_nodes.len(), 1, "Expected a single parent node");
        let parent_children = parent_nodes[0].fetch_children(&store).unwrap();
        assert_eq!(parent_children.len(), 3);
    }

    #[test]
    fn parent_nodes_respect_max_children() {
        let store = InMemoryStore::default();

        let mut keys: Vec<UUID> = (0..(MAX_CHILDREN + 7)).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        // Build sorted children with mixed duplicate and singleton runs.
        let mut children = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            children.push(HtreeNode::<u64>::from_kvp(key, &(i as u64), &store).unwrap());
            if i % 3 == 0 {
                children.push(HtreeNode::<u64>::from_kvp(key, &(i as u64 + 1000), &store).unwrap());
            }
        }

        let parent_nodes = HtreeNode::from_many_children(children, &store).unwrap();

        for parent in &parent_nodes {
            let count = parent.fetch_children(&store).unwrap().len();
            assert!(
                count <= MAX_CHILDREN,
                "Parent has {} children, expected <= {}",
                count,
                MAX_CHILDREN
            );
        }
    }

    #[test]
    fn duplicate_run_exceeds_max_children_splits() {
        let store = InMemoryStore::default();

        let key = UUID::gen_v4();
        let num_children = MAX_CHILDREN * 2 + 1;

        let children: Vec<_> = (0..num_children)
            .map(|i| HtreeNode::<u64>::from_kvp(&key, &(i as u64), &store).unwrap())
            .collect();

        let parent_nodes = HtreeNode::from_many_children(children, &store).unwrap();

        let expected_groups = num_children.div_ceil(MAX_CHILDREN);
        assert_eq!(
            parent_nodes.len(),
            expected_groups,
            "Expected {} parent nodes for {} children",
            expected_groups,
            num_children
        );

        let total_children: usize = parent_nodes
            .iter()
            .map(|p| p.fetch_children(&store).unwrap().len())
            .sum();
        assert_eq!(total_children, num_children);
    }

    #[test]
    fn empty_children_returns_empty() {
        let store = InMemoryStore::default();
        let children: Vec<HtreeNode<u64>> = Vec::new();
        let parent_nodes = HtreeNode::from_many_children(children, &store).unwrap();
        assert!(parent_nodes.is_empty());
    }

    #[test]
    fn mixed_runs_under_max_children_stay_single_parent() {
        let store = InMemoryStore::default();

        // Construct: A(1x), B(3x), C(1x), D(2x) with total <= MAX_CHILDREN
        let mut keys: Vec<UUID> = (0..4).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let mut children = Vec::new();
        children.push(HtreeNode::<u64>::from_kvp(&keys[0], &0, &store).unwrap());
        for i in 0..3 {
            children.push(HtreeNode::<u64>::from_kvp(&keys[1], &(10 + i), &store).unwrap());
        }
        children.push(HtreeNode::<u64>::from_kvp(&keys[2], &20, &store).unwrap());
        for i in 0..2 {
            children.push(HtreeNode::<u64>::from_kvp(&keys[3], &(30 + i), &store).unwrap());
        }

        assert!(children.len() <= MAX_CHILDREN);
        let parent_nodes = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parent_nodes.len(), 1, "Expected a single parent node");
        let parent_children = parent_nodes[0].fetch_children(&store).unwrap();
        assert_eq!(parent_children.len(), 7);
    }
}
