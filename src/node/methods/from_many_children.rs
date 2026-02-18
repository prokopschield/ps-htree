use std::mem;

use ps_hkey::Store;

use crate::{HtreeNode, HtreeNodeFromChildrenError, MAX_CHILDREN};

impl<T> HtreeNode<T> {
    /// Constructs parent nodes from an iterator of child nodes.
    ///
    /// Sorts children, increments height, groups into chunks of at most
    /// [`MAX_CHILDREN`], and returns one or more parent nodes. Children with
    /// the same key are always kept in the same parent node to preserve key
    /// boundary invariants for tree operations.
    ///
    /// # Algorithm
    ///
    /// The function groups consecutive children by key pattern:
    /// - **Duplicate runs**: consecutive children sharing the same key
    /// - **Unique runs**: consecutive children with distinct keys
    ///
    /// Runs are flushed and converted to parent nodes when:
    /// - A pattern transition occurs (unique to duplicate or vice versa)
    /// - A duplicate run exceeds [`MAX_CHILDREN`] and a new key arrives
    ///
    /// When a run exceeds [`MAX_CHILDREN`], it is split evenly across multiple
    /// parent nodes. Unique runs are buffered until complete to enable optimal
    /// even splitting.
    ///
    /// # Arguments
    ///
    /// * `children` - Child nodes to group. May be in any order.
    /// * `store` - The backing store for persisting node data.
    ///
    /// # Errors
    ///
    /// Returns [`HtreeNodeFromChildrenError`] if:
    /// - Child heights are inconsistent (see [`HtreeNodeFromChildrenError::ChildHeightInconsistent`])
    /// - Node height would overflow (see [`HtreeNodeFromChildrenError::HeightOverflow`])
    /// - Store operations fail (see [`HtreeNodeFromChildrenError::Store`])
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Children with the same key stay together
    /// let children = vec![leaf_a1, leaf_a2, leaf_b1, leaf_b2];
    /// let parents = HtreeNode::from_many_children(children, &store)?;
    /// // All children with key A are in the same parent node
    /// ```
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
        let mut nodes = Vec::with_capacity(children.len().div_ceil(MAX_CHILDREN));

        for child in children {
            // Need at least 2 elements to determine run type
            if buf.len() <= 1 {
                buf.push(child);
                continue;
            }

            // Detect the current buffer's "run type" and whether the incoming
            // child continues the last key.
            //
            // Run types:
            // - Duplicate run: all children share the same key (buf[0].key == buf[1].key)
            // - Unique run: children have distinct keys (buf[0].key != buf[1].key)
            let buf_is_duplicate_run = buf[0].key == buf[1].key;
            let child_continues_last = buf.last().is_some_and(|last| last.key == child.key);

            // Flush an oversized duplicate run once we see a new key.
            // Keep unique runs buffered so they can be split evenly in `combine_group`.
            if buf_is_duplicate_run && buf.len() >= MAX_CHILDREN && !child_continues_last {
                nodes.extend(combine_group(mem::take(&mut buf), store)?);
                buf.push(child);
                continue;
            }

            // Continue buffering if pattern is consistent:
            // - Duplicate run + same key continues = still a duplicate run
            // - Unique run + new key = still a unique run
            if buf_is_duplicate_run == child_continues_last {
                buf.push(child);
                continue;
            }

            // Pattern changed: transitioning between duplicate and unique runs.
            // If child_continues_last is true, the last buffer item actually belongs
            // with this child (starting a new duplicate run), so hold it back.
            let held_child = if child_continues_last {
                buf.pop()
            } else {
                None
            };

            nodes.extend(combine_group(mem::take(&mut buf), store)?);

            if let Some(c) = held_child {
                buf.push(c);
            }

            buf.push(child);
        }

        if !buf.is_empty() {
            nodes.extend(combine_group(buf, store)?);
        }

        Ok(nodes)
    }
}

/// Combines children into one or more parent nodes, splitting evenly if needed.
///
/// When `children.len() > MAX_CHILDREN`, splits them into approximately equal
/// groups to ensure balanced distribution across parent nodes.
fn combine_group<T, S>(
    children: Vec<HtreeNode<T>>,
    store: &S,
) -> Result<Vec<HtreeNode<T>>, HtreeNodeFromChildrenError<S>>
where
    S: Store,
{
    let num_children = children.len();

    if num_children <= MAX_CHILDREN {
        return Ok(vec![HtreeNode::from_sorted_children(children, store)?]);
    }

    let num_groups = num_children.div_ceil(MAX_CHILDREN);
    let group_size = num_children.div_ceil(num_groups);

    let mut children = children.into_iter();
    let mut groups = Vec::with_capacity(num_groups);

    for _ in 0..num_groups {
        groups.push(HtreeNode::from_sorted_children(
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

    // ==================== Helper functions ====================

    fn make_leaf(key: &UUID, value: u64, store: &InMemoryStore) -> HtreeNode<u64> {
        HtreeNode::from_kvp(key, &value, store).unwrap()
    }

    fn sorted_keys(n: usize) -> Vec<UUID> {
        let mut keys: Vec<UUID> = (0..n).map(|_| UUID::gen_v4()).collect();
        keys.sort();
        keys
    }

    fn count_children(parent: &HtreeNode<u64>, store: &InMemoryStore) -> usize {
        parent.fetch_children(store).unwrap().len()
    }

    fn total_children(parents: &[HtreeNode<u64>], store: &InMemoryStore) -> usize {
        parents.iter().map(|p| count_children(p, store)).sum()
    }

    fn parent_group_sizes(parents: &[HtreeNode<u64>], store: &InMemoryStore) -> Vec<usize> {
        parents.iter().map(|p| count_children(p, store)).collect()
    }

    fn expected_even_group_sizes(num_children: usize) -> Vec<usize> {
        let num_groups = num_children.div_ceil(MAX_CHILDREN);
        let group_size = num_children.div_ceil(num_groups);
        let mut remaining = num_children;
        let mut sizes = Vec::with_capacity(num_groups);

        for _ in 0..num_groups {
            let take = remaining.min(group_size);
            sizes.push(take);
            remaining -= take;
        }

        sizes
    }

    /// Verifies that no key appears in multiple non-consecutive parent nodes.
    /// Keys may span multiple consecutive parents (when > MAX_CHILDREN), but must be contiguous.
    fn assert_keys_contiguous(parents: &[HtreeNode<u64>], store: &InMemoryStore) {
        // Track the range of parent indices where each key appears
        let mut key_ranges: HashMap<UUID, (usize, usize)> = HashMap::new();

        for (idx, parent) in parents.iter().enumerate() {
            for child in parent.fetch_children(store).unwrap() {
                key_ranges
                    .entry(child.key)
                    .and_modify(|(_, end)| *end = idx)
                    .or_insert((idx, idx));
            }
        }

        // Verify each key only appears in its declared range
        for (idx, parent) in parents.iter().enumerate() {
            for child in parent.fetch_children(store).unwrap() {
                let (start, end) = key_ranges[&child.key];
                assert!(
                    idx >= start && idx <= end,
                    "Key {:?} appears in parent {} but range is [{}, {}]",
                    child.key,
                    idx,
                    start,
                    end
                );
            }
        }
    }

    /// Verifies that no key appears in multiple parent nodes (strict version).
    /// Use this only when each key has <= MAX_CHILDREN children.
    fn assert_keys_not_split(parents: &[HtreeNode<u64>], store: &InMemoryStore) {
        let mut key_to_parent: HashMap<UUID, usize> = HashMap::new();
        for (idx, parent) in parents.iter().enumerate() {
            for child in parent.fetch_children(store).unwrap() {
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

    /// Verifies all parents have at most MAX_CHILDREN.
    fn assert_max_children_respected(parents: &[HtreeNode<u64>], store: &InMemoryStore) {
        for (i, parent) in parents.iter().enumerate() {
            let count = count_children(parent, store);
            assert!(
                count <= MAX_CHILDREN,
                "Parent {} has {} children, expected <= {}",
                i,
                count,
                MAX_CHILDREN
            );
        }
    }

    // ==================== Edge cases: small inputs ====================

    #[test]
    fn empty_children_returns_empty() {
        let store = InMemoryStore::default();
        let children: Vec<HtreeNode<u64>> = Vec::new();
        let parents = HtreeNode::from_many_children(children, &store).unwrap();
        assert!(parents.is_empty());
    }

    #[test]
    fn single_child() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let children = vec![make_leaf(&key, 1, &store)];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 1);
    }

    #[test]
    fn two_children_same_key() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let children = vec![make_leaf(&key, 1, &store), make_leaf(&key, 2, &store)];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 2);
    }

    #[test]
    fn two_children_different_keys() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);
        let children = vec![
            make_leaf(&keys[0], 1, &store),
            make_leaf(&keys[1], 2, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 2);
    }

    #[test]
    fn three_children_all_same_key() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let children = vec![
            make_leaf(&key, 1, &store),
            make_leaf(&key, 2, &store),
            make_leaf(&key, 3, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 3);
    }

    #[test]
    fn three_children_all_different_keys() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(3);
        let children = vec![
            make_leaf(&keys[0], 1, &store),
            make_leaf(&keys[1], 2, &store),
            make_leaf(&keys[2], 3, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 3);
    }

    // ==================== Boundary cases: around MAX_CHILDREN ====================

    #[test]
    fn exactly_max_children_unique_keys() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(
            parents.len(),
            1,
            "Exactly MAX_CHILDREN should fit in one node"
        );
        assert_eq!(count_children(&parents[0], &store), MAX_CHILDREN);
    }

    #[test]
    fn exactly_max_children_same_key() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let children: Vec<_> = (0..MAX_CHILDREN)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(
            parents.len(),
            1,
            "Exactly MAX_CHILDREN same-key should fit in one node"
        );
        assert_eq!(count_children(&parents[0], &store), MAX_CHILDREN);
    }

    #[test]
    fn max_children_plus_one_unique_keys() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 1);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(
            parents.len(),
            2,
            "MAX_CHILDREN + 1 unique should split into 2 nodes"
        );
        assert_eq!(
            parent_group_sizes(&parents, &store),
            expected_even_group_sizes(MAX_CHILDREN + 1)
        );
        assert_eq!(total_children(&parents, &store), MAX_CHILDREN + 1);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn max_children_plus_one_same_key() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let children: Vec<_> = (0..MAX_CHILDREN + 1)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(
            parents.len(),
            2,
            "MAX_CHILDREN + 1 same-key should split into 2 nodes"
        );
        assert_eq!(total_children(&parents, &store), MAX_CHILDREN + 1);
    }

    // ==================== Pattern transitions ====================

    #[test]
    fn unique_then_duplicate_under_max() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);

        // A(1x), B(2x) -> total 3, should stay in one group
        let children = vec![
            make_leaf(&keys[0], 0, &store),
            make_leaf(&keys[1], 1, &store),
            make_leaf(&keys[1], 2, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 3);
    }

    #[test]
    fn duplicate_then_unique_under_max() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);

        // A(2x), B(1x) -> total 3, should stay in one group
        let children = vec![
            make_leaf(&keys[0], 0, &store),
            make_leaf(&keys[0], 1, &store),
            make_leaf(&keys[1], 2, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 3);
    }

    #[test]
    fn singleton_before_duplicates_over_max() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        // A(1x), B(2x), C(1x), D(1x), ... until we exceed MAX_CHILDREN
        let mut children = Vec::new();
        children.push(make_leaf(&keys[0], 0, &store)); // singleton
        children.push(make_leaf(&keys[1], 1, &store)); // duplicate start
        children.push(make_leaf(&keys[1], 2, &store)); // duplicate
        for (i, key) in keys.iter().enumerate().skip(2) {
            children.push(make_leaf(key, i as u64, &store));
        }

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn duplicates_then_singletons_over_max() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        // A(3x), then unique keys until we exceed MAX_CHILDREN
        let mut children = Vec::new();
        for i in 0..3 {
            children.push(make_leaf(&keys[0], i, &store));
        }
        for (i, key) in keys.iter().enumerate().skip(1) {
            children.push(make_leaf(key, (i + 10) as u64, &store));
        }

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn alternating_singleton_duplicate_runs() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 5);

        // A(1x), B(2x), C(1x), D(2x), E(1x), ...
        let mut children = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            children.push(make_leaf(key, i as u64, &store));
            if i % 2 == 1 {
                children.push(make_leaf(key, (i + 100) as u64, &store));
            }
        }

        let num_children = children.len();
        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
        assert_eq!(total_children(&parents, &store), num_children);
    }

    // ==================== Large duplicate runs ====================

    #[test]
    fn all_same_key_double_max() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let num = MAX_CHILDREN * 2;
        let children: Vec<_> = (0..num)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 2);
        assert_eq!(total_children(&parents, &store), num);
    }

    #[test]
    fn all_same_key_triple_max_plus_one() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let num = MAX_CHILDREN * 3 + 1;
        let children: Vec<_> = (0..num)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        let expected_groups = num.div_ceil(MAX_CHILDREN);
        assert_eq!(parents.len(), expected_groups);
        assert_eq!(total_children(&parents, &store), num);
    }

    // ==================== Unique keys large scale ====================

    #[test]
    fn unique_keys_double_max() {
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN * 2;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), num.div_ceil(MAX_CHILDREN));
        assert_eq!(
            parent_group_sizes(&parents, &store),
            expected_even_group_sizes(num)
        );
        assert_eq!(total_children(&parents, &store), num);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn unique_keys_triple_max_plus_partial() {
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN * 3 + 5;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), num.div_ceil(MAX_CHILDREN));
        assert_eq!(
            parent_group_sizes(&parents, &store),
            expected_even_group_sizes(num)
        );
        assert_eq!(total_children(&parents, &store), num);
        assert_max_children_respected(&parents, &store);
    }

    // ==================== Mixed patterns ====================

    #[test]
    fn mixed_runs_under_max_children() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(4);

        // A(1x), B(3x), C(1x), D(2x) = 7 total
        let mut children = Vec::new();
        children.push(make_leaf(&keys[0], 0, &store));
        for i in 0..3 {
            children.push(make_leaf(&keys[1], (10 + i) as u64, &store));
        }
        children.push(make_leaf(&keys[2], 20, &store));
        for i in 0..2 {
            children.push(make_leaf(&keys[3], (30 + i) as u64, &store));
        }

        assert!(children.len() <= MAX_CHILDREN);
        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 7);
    }

    #[test]
    fn mixed_duplicate_and_singleton_over_max() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 7);

        let mut children = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            children.push(make_leaf(key, i as u64, &store));
            if i % 3 == 0 {
                children.push(make_leaf(key, (i + 1000) as u64, &store));
            }
        }

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn same_key_children_stay_in_same_node() {
        let store = InMemoryStore::default();

        let num_children = MAX_CHILDREN * 2 + 5;
        let num_unique_keys = 10;

        let mut keys = sorted_keys(num_unique_keys);
        keys.sort(); // already sorted but explicit

        let children: Vec<_> = (0..num_children)
            .map(|i| make_leaf(&keys[i % num_unique_keys], i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_keys_not_split(&parents, &store);
    }

    // ==================== Key preservation ====================

    #[test]
    fn preserves_all_keys_and_counts() {
        let store = InMemoryStore::default();

        let num_children = MAX_CHILDREN * 3;
        let num_unique_keys = 20;
        let keys = sorted_keys(num_unique_keys);

        let mut expected_key_counts: HashMap<UUID, usize> = HashMap::new();
        let children: Vec<_> = (0..num_children)
            .map(|i| {
                let key = keys[i % num_unique_keys];
                *expected_key_counts.entry(key).or_insert(0) += 1;
                make_leaf(&key, i as u64, &store)
            })
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        let mut actual_key_counts: HashMap<UUID, usize> = HashMap::new();
        for parent in &parents {
            for child in parent.fetch_children(&store).unwrap() {
                *actual_key_counts.entry(child.key).or_insert(0) += 1;
            }
        }

        assert_eq!(expected_key_counts, actual_key_counts);
    }

    // ==================== Ordering ====================

    #[test]
    fn output_nodes_are_sorted_by_key() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN * 2);

        // Intentionally create children out of order
        let mut children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();
        children.reverse();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        // Each parent's key should be <= the next parent's key
        for i in 1..parents.len() {
            assert!(
                parents[i - 1].key <= parents[i].key,
                "Parents not sorted: {:?} > {:?}",
                parents[i - 1].key,
                parents[i].key
            );
        }
    }

    #[test]
    fn children_within_parent_are_sorted() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN * 2);

        let mut children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();
        children.reverse();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        for parent in &parents {
            let children = parent.fetch_children(&store).unwrap();
            for i in 1..children.len() {
                assert!(
                    children[i - 1].key <= children[i].key,
                    "Children not sorted: {:?} > {:?}",
                    children[i - 1].key,
                    children[i].key
                );
            }
        }
    }

    // ==================== Edge cases with specific patterns ====================

    #[test]
    fn long_duplicate_run_at_start() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN / 2 + 5);

        // Big duplicate run at start, then unique keys
        let mut children = Vec::new();
        for i in 0..MAX_CHILDREN {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        for (i, key) in keys.iter().enumerate().skip(1) {
            children.push(make_leaf(key, (i + 1000) as u64, &store));
        }

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn long_duplicate_run_at_end() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN / 2 + 5);

        // Unique keys first, then big duplicate run at end
        let mut children = Vec::new();
        for (i, key) in keys.iter().enumerate().take(keys.len() - 1) {
            children.push(make_leaf(key, i as u64, &store));
        }
        let last_key = keys.last().unwrap();
        for i in 0..MAX_CHILDREN {
            children.push(make_leaf(last_key, (i + 1000) as u64, &store));
        }

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn duplicate_run_spanning_boundary() {
        let store = InMemoryStore::default();
        let keys = sorted_keys(3);

        // Construct so that a duplicate run would naturally span a MAX_CHILDREN boundary
        // Fill almost to MAX_CHILDREN with first key, then second key has duplicates
        let mut children = Vec::new();
        for i in 0..(MAX_CHILDREN - 2) {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        // Now add 5 of keys[1] which would span the boundary
        for i in 0..5 {
            children.push(make_leaf(&keys[1], (i + 1000) as u64, &store));
        }
        // Add some of keys[2]
        for i in 0..3 {
            children.push(make_leaf(&keys[2], (i + 2000) as u64, &store));
        }

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    // ==================== Large scale stress tests (>10k records) ====================

    #[test]
    fn stress_10k_unique_keys() {
        let store = InMemoryStore::default();
        let num = 10_000;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn stress_10k_same_key() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let num: usize = 10_000;
        let children: Vec<_> = (0..num)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        let expected_groups = num.div_ceil(MAX_CHILDREN);
        assert_eq!(parents.len(), expected_groups);
        assert_eq!(total_children(&parents, &store), num);
    }

    #[test]
    fn stress_50k_mixed_keys() {
        let store = InMemoryStore::default();
        let num_children = 50_000;
        let num_unique_keys = 1_000;
        let keys = sorted_keys(num_unique_keys);

        // 50 children per key - may exceed MAX_CHILDREN depending on config
        let children: Vec<_> = (0..num_children)
            .map(|i| make_leaf(&keys[i % num_unique_keys], i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_contiguous(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn stress_100k_alternating_pattern() {
        let store = InMemoryStore::default();
        let num_keys = 20_000;
        let keys = sorted_keys(num_keys);

        // Alternating: 1 child, 2 children, 1 child, 2 children, ...
        let mut children = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            children.push(make_leaf(key, i as u64, &store));
            if i % 2 == 1 {
                children.push(make_leaf(key, (i + 100_000) as u64, &store));
            }
        }

        let num_children = children.len();
        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn stress_large_duplicate_runs_interspersed() {
        let store = InMemoryStore::default();
        let num_keys = 100;
        let keys = sorted_keys(num_keys);

        // Each key gets 500 children = 50,000 total
        // 500 > MAX_CHILDREN, so keys will span multiple parents
        let mut children = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            for j in 0..500 {
                children.push(make_leaf(key, (i * 1000 + j) as u64, &store));
            }
        }

        let num_children = children.len();
        assert_eq!(num_children, 50_000);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_contiguous(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn stress_worst_case_many_transitions() {
        let store = InMemoryStore::default();
        // Create pattern: A, B, B, C, D, D, E, F, F, ...
        // This forces many transitions between unique and duplicate runs
        let num_pairs = 5_000;
        let keys = sorted_keys(num_pairs * 2);

        let mut children = Vec::new();
        for i in 0..num_pairs {
            // Single child
            children.push(make_leaf(&keys[i * 2], (i * 10) as u64, &store));
            // Duplicate pair
            children.push(make_leaf(&keys[i * 2 + 1], (i * 10 + 1) as u64, &store));
            children.push(make_leaf(&keys[i * 2 + 1], (i * 10 + 2) as u64, &store));
        }

        let num_children = children.len();
        assert_eq!(num_children, 15_000);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    // ==================== Additional edge case tests ====================

    #[test]
    fn three_children_two_same_one_different_aab() {
        // Pattern: A, A, B (duplicate then singleton)
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);
        let children = vec![
            make_leaf(&keys[0], 0, &store),
            make_leaf(&keys[0], 1, &store),
            make_leaf(&keys[1], 2, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 3);
    }

    #[test]
    fn three_children_two_same_one_different_abb() {
        // Pattern: A, B, B (singleton then duplicate)
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);
        let children = vec![
            make_leaf(&keys[0], 0, &store),
            make_leaf(&keys[1], 1, &store),
            make_leaf(&keys[1], 2, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 3);
    }

    #[test]
    fn four_children_aabb() {
        // Pattern: A, A, B, B (two pairs)
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);
        let children = vec![
            make_leaf(&keys[0], 0, &store),
            make_leaf(&keys[0], 1, &store),
            make_leaf(&keys[1], 2, &store),
            make_leaf(&keys[1], 3, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 4);
    }

    #[test]
    fn four_children_abab_becomes_aabb_after_sort() {
        // Input out of order: A, B, A, B -> after sort: A, A, B, B
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);
        let children = vec![
            make_leaf(&keys[0], 0, &store),
            make_leaf(&keys[1], 1, &store),
            make_leaf(&keys[0], 2, &store),
            make_leaf(&keys[1], 3, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 4);
        assert_keys_not_split(&parents, &store);
    }

    #[test]
    fn five_children_aabbc() {
        // Pattern: A, A, B, B, C
        let store = InMemoryStore::default();
        let keys = sorted_keys(3);
        let children = vec![
            make_leaf(&keys[0], 0, &store),
            make_leaf(&keys[0], 1, &store),
            make_leaf(&keys[1], 2, &store),
            make_leaf(&keys[1], 3, &store),
            make_leaf(&keys[2], 4, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 5);
    }

    #[test]
    fn unique_run_exactly_max_children_then_duplicate() {
        // MAX_CHILDREN unique keys, then one key repeated (starts duplicate run)
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 1);

        let mut children = Vec::new();
        // MAX_CHILDREN unique keys
        for (i, key) in keys.iter().take(MAX_CHILDREN).enumerate() {
            children.push(make_leaf(key, i as u64, &store));
        }
        // One more key repeated twice
        children.push(make_leaf(&keys[MAX_CHILDREN], 1000, &store));
        children.push(make_leaf(&keys[MAX_CHILDREN], 1001, &store));

        let num_children = children.len();
        assert_eq!(num_children, MAX_CHILDREN + 2);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn duplicate_run_exactly_max_children_then_unique() {
        // One key repeated MAX_CHILDREN times, then unique keys
        let store = InMemoryStore::default();
        let keys = sorted_keys(5);

        let mut children = Vec::new();
        // MAX_CHILDREN of the same key
        for i in 0..MAX_CHILDREN {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        // Then some unique keys
        for (i, key) in keys.iter().enumerate().skip(1) {
            children.push(make_leaf(key, (1000 + i) as u64, &store));
        }

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn max_children_minus_one_unique_then_pair() {
        // (MAX_CHILDREN - 1) unique keys, then one key as a pair
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        let mut children = Vec::new();
        for (i, key) in keys.iter().take(MAX_CHILDREN - 1).enumerate() {
            children.push(make_leaf(key, i as u64, &store));
        }
        // Last key appears twice
        children.push(make_leaf(&keys[MAX_CHILDREN - 1], 1000, &store));
        children.push(make_leaf(&keys[MAX_CHILDREN - 1], 1001, &store));

        let num_children = children.len();
        assert_eq!(num_children, MAX_CHILDREN + 1);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn exactly_two_max_children_all_unique() {
        // Exactly 2 * MAX_CHILDREN unique keys
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN * 2;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        // Should split into exactly 2 groups
        assert_eq!(parents.len(), 2);
        assert_eq!(
            parent_group_sizes(&parents, &store),
            vec![MAX_CHILDREN, MAX_CHILDREN]
        );
    }

    #[test]
    fn exactly_two_max_children_plus_one_unique() {
        // 2 * MAX_CHILDREN + 1 unique keys
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN * 2 + 1;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        // Should split into 3 groups with even distribution
        assert_eq!(parents.len(), 3);
        assert_eq!(total_children(&parents, &store), num);
        assert_eq!(
            parent_group_sizes(&parents, &store),
            expected_even_group_sizes(num)
        );
    }

    #[test]
    fn single_key_exactly_max_children() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let children: Vec<_> = (0..MAX_CHILDREN)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), MAX_CHILDREN);
    }

    #[test]
    fn single_key_max_children_plus_one() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let children: Vec<_> = (0..MAX_CHILDREN + 1)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        // Should split evenly
        assert_eq!(parents.len(), 2);
        assert_eq!(
            parent_group_sizes(&parents, &store),
            expected_even_group_sizes(MAX_CHILDREN + 1)
        );
    }

    #[test]
    fn single_key_exactly_two_max_children() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let children: Vec<_> = (0..MAX_CHILDREN * 2)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 2);
        assert_eq!(
            parent_group_sizes(&parents, &store),
            vec![MAX_CHILDREN, MAX_CHILDREN]
        );
    }

    #[test]
    fn two_keys_split_evenly() {
        // Two keys, each with MAX_CHILDREN / 2 + 1 children
        // Total > MAX_CHILDREN, should split
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);
        let per_key = MAX_CHILDREN / 2 + 1;

        let mut children = Vec::new();
        for i in 0..per_key {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        for i in 0..per_key {
            children.push(make_leaf(&keys[1], (1000 + i) as u64, &store));
        }

        let num_children = children.len();
        assert!(num_children > MAX_CHILDREN);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn many_small_duplicate_runs() {
        // Many keys, each with exactly 2 children
        let store = InMemoryStore::default();
        let num_keys = MAX_CHILDREN; // Total children = MAX_CHILDREN * 2
        let keys = sorted_keys(num_keys);

        let mut children = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            children.push(make_leaf(key, (i * 10) as u64, &store));
            children.push(make_leaf(key, (i * 10 + 1) as u64, &store));
        }

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn many_small_duplicate_runs_exceeding_max() {
        // More keys than MAX_CHILDREN, each with 2 children
        let store = InMemoryStore::default();
        let num_keys = MAX_CHILDREN + 10;
        let keys = sorted_keys(num_keys);

        let mut children = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            children.push(make_leaf(key, (i * 10) as u64, &store));
            children.push(make_leaf(key, (i * 10 + 1) as u64, &store));
        }

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn transition_at_exact_max_children_boundary() {
        // Fill buffer to exactly MAX_CHILDREN with unique keys,
        // then next child starts a duplicate run
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 1);

        let mut children = Vec::new();
        // MAX_CHILDREN - 1 unique keys
        for (i, key) in keys.iter().take(MAX_CHILDREN - 1).enumerate() {
            children.push(make_leaf(key, i as u64, &store));
        }
        // Then 3 of the last key (causes transition at boundary)
        for i in 0..3 {
            children.push(make_leaf(
                &keys[MAX_CHILDREN - 1],
                (1000 + i) as u64,
                &store,
            ));
        }

        let num_children = children.len();
        assert_eq!(num_children, MAX_CHILDREN + 2);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn reverse_order_input() {
        // Children provided in reverse sorted order
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN + 50;
        let keys = sorted_keys(num);

        let mut children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();
        children.reverse();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num);
        assert_max_children_respected(&parents, &store);

        // Verify output is sorted
        for i in 1..parents.len() {
            assert!(parents[i - 1].key <= parents[i].key);
        }
    }

    #[test]
    fn random_shuffle_input() {
        // Children in random order (simulated by interleaving)
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN + 50;
        let keys = sorted_keys(num);

        // Interleave first half with second half
        let mut children = Vec::new();
        let mid = num / 2;
        for i in 0..mid {
            children.push(make_leaf(&keys[i], i as u64, &store));
            if mid + i < num {
                children.push(make_leaf(&keys[mid + i], (mid + i) as u64, &store));
            }
        }

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn all_children_same_key_large() {
        // Large number of children all with the same key
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let num = MAX_CHILDREN * 5 + 7;
        let children: Vec<_> = (0..num)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        let expected_groups = num.div_ceil(MAX_CHILDREN);
        assert_eq!(parents.len(), expected_groups);
        assert_eq!(total_children(&parents, &store), num);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn pattern_singleton_triple_singleton() {
        // A, B, B, B, C pattern (singleton, triple, singleton)
        let store = InMemoryStore::default();
        let keys = sorted_keys(3);
        let children = vec![
            make_leaf(&keys[0], 0, &store),
            make_leaf(&keys[1], 1, &store),
            make_leaf(&keys[1], 2, &store),
            make_leaf(&keys[1], 3, &store),
            make_leaf(&keys[2], 4, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 5);
        assert_keys_not_split(&parents, &store);
    }

    #[test]
    fn pattern_triple_singleton_triple() {
        // A, A, A, B, C, C, C pattern
        let store = InMemoryStore::default();
        let keys = sorted_keys(3);
        let children = vec![
            make_leaf(&keys[0], 0, &store),
            make_leaf(&keys[0], 1, &store),
            make_leaf(&keys[0], 2, &store),
            make_leaf(&keys[1], 3, &store),
            make_leaf(&keys[2], 4, &store),
            make_leaf(&keys[2], 5, &store),
            make_leaf(&keys[2], 6, &store),
        ];

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), 7);
        assert_keys_not_split(&parents, &store);
    }

    #[test]
    fn children_already_sorted() {
        // Verify behavior when children are already in sorted order
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN + 20;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn boundary_max_children_minus_one() {
        // Exactly MAX_CHILDREN - 1 children (should not split)
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN - 1;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), num);
    }

    #[test]
    fn boundary_max_children_exact() {
        // Exactly MAX_CHILDREN children (should not split)
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(parents.len(), 1);
        assert_eq!(count_children(&parents[0], &store), num);
    }

    #[test]
    fn one_large_run_then_many_unique() {
        // One key with MAX_CHILDREN children, then many unique keys
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 1);

        let mut children = Vec::new();
        // First key gets MAX_CHILDREN children
        for i in 0..MAX_CHILDREN {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        // Then MAX_CHILDREN unique keys
        for (i, key) in keys.iter().enumerate().skip(1) {
            children.push(make_leaf(key, (1000 + i) as u64, &store));
        }

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn verify_parent_keys_are_minimum_child_keys() {
        // Verify that each parent's key equals its minimum child key
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN * 2 + 50;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        for parent in &parents {
            let children = parent.fetch_children(&store).unwrap();
            let min_child_key = children.iter().map(|c| c.key).min().unwrap();
            assert_eq!(
                parent.key, min_child_key,
                "Parent key should be minimum child key"
            );
        }
    }

    #[test]
    fn verify_all_children_have_same_height() {
        // All children should have the same height
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN + 10;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        // All parents should have height 1 (leaves are height 0)
        for parent in &parents {
            assert_eq!(parent.height, 1);
        }
    }

    #[test]
    fn adjacent_duplicate_runs_both_over_max() {
        // Two adjacent keys, each with > MAX_CHILDREN children
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);
        let per_key = MAX_CHILDREN + 50;

        let mut children = Vec::new();
        for i in 0..per_key {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        for i in 0..per_key {
            children.push(make_leaf(&keys[1], (1000 + i) as u64, &store));
        }

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_contiguous(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    // ==================== Buffer boundary edge cases ====================

    #[test]
    fn unique_run_exactly_max_children_elements_then_pattern_change() {
        // Buffer has exactly MAX_CHILDREN unique elements, then pattern changes
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 1);

        let mut children = Vec::new();
        // MAX_CHILDREN unique keys
        for (i, key) in keys.iter().take(MAX_CHILDREN).enumerate() {
            children.push(make_leaf(key, i as u64, &store));
        }
        // Last key appears twice (triggers pattern change)
        children.push(make_leaf(&keys[MAX_CHILDREN - 1], 1000, &store));

        let num_children = children.len();
        assert_eq!(num_children, MAX_CHILDREN + 1);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn duplicate_run_exactly_max_children_then_new_key() {
        // Buffer has exactly MAX_CHILDREN same-key elements, then new key
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);

        let mut children = Vec::new();
        for i in 0..MAX_CHILDREN {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        children.push(make_leaf(&keys[1], 1000, &store));

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn duplicate_run_max_children_plus_one_then_new_key() {
        // Buffer grows beyond MAX_CHILDREN (same key), then new key triggers flush
        // Note: key[0] has > MAX_CHILDREN children, so it WILL span multiple parents
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);

        let mut children = Vec::new();
        for i in 0..MAX_CHILDREN + 1 {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        children.push(make_leaf(&keys[1], 1000, &store));

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        // Use assert_keys_contiguous because key[0] has > MAX_CHILDREN children
        assert_keys_contiguous(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn buffer_with_two_elements_transition_to_duplicate() {
        // Minimal case: [A, B] then B comes (transition to duplicate)
        // This only happens when > MAX_CHILDREN total
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        let mut children = Vec::new();
        // First two unique keys
        children.push(make_leaf(&keys[0], 0, &store));
        children.push(make_leaf(&keys[1], 1, &store));
        // Second key repeated (starts duplicate)
        children.push(make_leaf(&keys[1], 2, &store));
        // Fill with more unique keys to exceed MAX_CHILDREN
        for (i, key) in keys.iter().enumerate().skip(2) {
            children.push(make_leaf(key, (100 + i) as u64, &store));
        }

        let num_children = children.len();
        assert!(num_children > MAX_CHILDREN);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn buffer_with_two_elements_transition_to_unique() {
        // Minimal case: [A, A] then B comes (transition to unique)
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        let mut children = Vec::new();
        // First key twice (duplicate run)
        children.push(make_leaf(&keys[0], 0, &store));
        children.push(make_leaf(&keys[0], 1, &store));
        // Fill with unique keys to exceed MAX_CHILDREN
        for (i, key) in keys.iter().enumerate().skip(1) {
            children.push(make_leaf(key, (100 + i) as u64, &store));
        }

        let num_children = children.len();
        assert!(num_children > MAX_CHILDREN);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn held_child_scenario() {
        // Test the held_child logic explicitly:
        // unique run [A, B, C], then C comes again
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        let mut children = Vec::new();
        // Three unique keys
        children.push(make_leaf(&keys[0], 0, &store));
        children.push(make_leaf(&keys[1], 1, &store));
        children.push(make_leaf(&keys[2], 2, &store));
        // Third key repeated (C should be held back)
        children.push(make_leaf(&keys[2], 3, &store));
        // More children to exceed MAX_CHILDREN
        for (i, key) in keys.iter().enumerate().skip(3) {
            children.push(make_leaf(key, (100 + i) as u64, &store));
        }

        let num_children = children.len();
        assert!(num_children > MAX_CHILDREN);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn no_held_child_scenario() {
        // Test when held_child is None:
        // duplicate run [A, A, A], then B comes
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        let mut children = Vec::new();
        // Three of the same key
        children.push(make_leaf(&keys[0], 0, &store));
        children.push(make_leaf(&keys[0], 1, &store));
        children.push(make_leaf(&keys[0], 2, &store));
        // Different key (no held_child needed)
        children.push(make_leaf(&keys[1], 100, &store));
        // More children to exceed MAX_CHILDREN
        for (i, key) in keys.iter().enumerate().skip(2) {
            children.push(make_leaf(key, (200 + i) as u64, &store));
        }

        let num_children = children.len();
        assert!(num_children > MAX_CHILDREN);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn multiple_transitions_in_sequence() {
        // Multiple rapid transitions: A, B, B, C, D, D, E, F, F, ...
        let store = InMemoryStore::default();
        let num_keys = MAX_CHILDREN + 20;
        let keys = sorted_keys(num_keys);

        let mut children = Vec::new();
        for (i, key) in keys.iter().enumerate() {
            children.push(make_leaf(key, (i * 10) as u64, &store));
            // Every third key gets a duplicate
            if i % 3 == 1 {
                children.push(make_leaf(key, (i * 10 + 1) as u64, &store));
            }
        }

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn flush_condition_boundary_duplicate_run_at_max() {
        // Duplicate run reaches exactly MAX_CHILDREN, then new key
        // This tests the flush condition: buf_is_duplicate_run && buf.len() >= MAX_CHILDREN && !child_continues_last
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);

        let mut children = Vec::new();
        // Exactly MAX_CHILDREN of key[0]
        for i in 0..MAX_CHILDREN {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        // Then key[1] - should trigger flush
        children.push(make_leaf(&keys[1], 1000, &store));

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
        // Should create 2 parents: one with MAX_CHILDREN, one with 1
        assert_eq!(parents.len(), 2);
    }

    #[test]
    fn flush_condition_boundary_duplicate_run_just_under_max() {
        // Duplicate run has MAX_CHILDREN - 1 elements, then new key
        // Should NOT trigger early flush (goes to pattern transition instead)
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        let mut children = Vec::new();
        // MAX_CHILDREN - 1 of key[0]
        for i in 0..(MAX_CHILDREN - 1) {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        // Then unique keys to exceed MAX_CHILDREN
        for (i, key) in keys.iter().enumerate().skip(1) {
            children.push(make_leaf(key, (1000 + i) as u64, &store));
        }

        let num_children = children.len();
        assert!(num_children > MAX_CHILDREN);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn verify_no_empty_parents() {
        // Ensure no parent has zero children
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN * 3 + 17;
        let keys = sorted_keys(num);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        for (i, parent) in parents.iter().enumerate() {
            let count = count_children(parent, &store);
            assert!(count > 0, "Parent {} has no children", i);
        }
    }

    #[test]
    fn verify_no_duplicate_children() {
        // Ensure each child appears exactly once across all parents
        let store = InMemoryStore::default();
        let num = MAX_CHILDREN * 2 + 50;
        let keys = sorted_keys(num);

        // Create children with unique values
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        // Collect all child values
        let mut all_values: Vec<u64> = Vec::new();
        for parent in &parents {
            for child in parent.fetch_children(&store).unwrap() {
                // Extract value by re-fetching
                all_values.push(child.key.as_bytes()[0] as u64); // Using key as proxy
            }
        }

        // Check total count matches
        assert_eq!(all_values.len(), num);
    }

    #[test]
    fn combine_group_edge_case_exactly_max_plus_one() {
        // Test combine_group splitting logic with MAX_CHILDREN + 1 elements
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let num = MAX_CHILDREN + 1;
        let children: Vec<_> = (0..num)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        // Should split into 2 groups with even distribution
        assert_eq!(parents.len(), 2);
        let sizes = parent_group_sizes(&parents, &store);
        assert_eq!(sizes, expected_even_group_sizes(num));
    }

    #[test]
    fn combine_group_edge_case_exactly_double_max() {
        // Test combine_group with exactly 2 * MAX_CHILDREN elements
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let num = MAX_CHILDREN * 2;
        let children: Vec<_> = (0..num)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        // Should split into exactly 2 groups of MAX_CHILDREN each
        assert_eq!(parents.len(), 2);
        assert_eq!(
            parent_group_sizes(&parents, &store),
            vec![MAX_CHILDREN, MAX_CHILDREN]
        );
    }

    #[test]
    fn combine_group_edge_case_double_max_plus_one() {
        // Test combine_group with 2 * MAX_CHILDREN + 1 elements
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let num = MAX_CHILDREN * 2 + 1;
        let children: Vec<_> = (0..num)
            .map(|i| make_leaf(&key, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        // Should split into 3 groups with even distribution
        assert_eq!(parents.len(), 3);
        assert_eq!(total_children(&parents, &store), num);
        // Each group should have roughly num/3 = 171 elements
        for size in parent_group_sizes(&parents, &store) {
            assert!(size <= MAX_CHILDREN);
        }
    }

    #[test]
    fn pattern_unique_ending_at_boundary_then_duplicate() {
        // Unique run ends exactly at element that becomes first of duplicate pair
        // [A, B, C, ..., X, Y] then Y again
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        let mut children = Vec::new();
        // MAX_CHILDREN - 1 unique keys
        for (i, key) in keys.iter().take(MAX_CHILDREN - 1).enumerate() {
            children.push(make_leaf(key, i as u64, &store));
        }
        // Last key twice
        children.push(make_leaf(&keys[MAX_CHILDREN - 1], 1000, &store));
        children.push(make_leaf(&keys[MAX_CHILDREN - 1], 1001, &store));

        let num_children = children.len();
        assert_eq!(num_children, MAX_CHILDREN + 1);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    // ==================== Algorithm-specific edge cases ====================

    #[test]
    fn duplicate_run_can_grow_beyond_max_while_same_key() {
        // Duplicate run grows beyond MAX_CHILDREN while same key continues
        // This is intentional: same-key children are kept together until a new key arrives
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);

        let mut children = Vec::new();
        // More than MAX_CHILDREN of the same key
        for i in 0..MAX_CHILDREN + 5 {
            children.push(make_leaf(&keys[0], i as u64, &store));
        }
        // Then a different key to trigger flush
        children.push(make_leaf(&keys[1], 1000, &store));

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        // key[0] will span multiple parents (> MAX_CHILDREN)
        assert_keys_contiguous(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn unique_run_can_grow_beyond_max_until_transition() {
        // Unique run grows beyond MAX_CHILDREN until pattern transitions to duplicate
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 10);

        let mut children = Vec::new();
        // Many unique keys
        for (i, key) in keys.iter().take(MAX_CHILDREN + 5).enumerate() {
            children.push(make_leaf(key, i as u64, &store));
        }
        // Then last key repeated (triggers transition and flush)
        children.push(make_leaf(&keys[MAX_CHILDREN + 4], 1000, &store));
        children.push(make_leaf(&keys[MAX_CHILDREN + 4], 1001, &store));

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn exactly_max_children_plus_one_with_last_key_repeated_twice() {
        // Exactly MAX_CHILDREN unique keys, plus last key repeated once more
        // Total: MAX_CHILDREN + 1 children
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        let mut children = Vec::new();
        // MAX_CHILDREN unique keys
        for (i, key) in keys.iter().enumerate() {
            children.push(make_leaf(key, i as u64, &store));
        }
        // Last key repeated once more
        children.push(make_leaf(&keys[MAX_CHILDREN - 1], 1000, &store));

        let num_children = children.len();
        assert_eq!(num_children, MAX_CHILDREN + 1);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn three_consecutive_pattern_transitions() {
        // A, B, B, C, D, D, E (singleton, pair, singleton, pair, singleton)
        // Forces three transitions in a row
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 5);

        let mut children = Vec::new();
        // Create the alternating pattern with enough total to exceed MAX_CHILDREN
        let mut child_count = 0;
        for (i, key) in keys.iter().enumerate() {
            children.push(make_leaf(key, (i * 10) as u64, &store));
            child_count += 1;
            if i % 2 == 1 {
                children.push(make_leaf(key, (i * 10 + 1) as u64, &store));
                child_count += 1;
            }
            if child_count > MAX_CHILDREN + 5 {
                break;
            }
        }

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn first_two_children_same_key() {
        // Edge case: first two children have same key, exceeds MAX_CHILDREN total
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        let mut children = Vec::new();
        // First key twice
        children.push(make_leaf(&keys[0], 0, &store));
        children.push(make_leaf(&keys[0], 1, &store));
        // Fill with unique keys
        for (i, key) in keys.iter().enumerate().skip(1) {
            children.push(make_leaf(key, (100 + i) as u64, &store));
        }

        let num_children = children.len();
        assert!(num_children > MAX_CHILDREN);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn last_two_children_same_key() {
        // Edge case: last two children have same key, exceeds MAX_CHILDREN total
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);

        let mut children = Vec::new();
        // Unique keys first
        for (i, key) in keys.iter().enumerate().take(MAX_CHILDREN - 1) {
            children.push(make_leaf(key, i as u64, &store));
        }
        // Last key twice
        children.push(make_leaf(&keys[MAX_CHILDREN - 1], 1000, &store));
        children.push(make_leaf(&keys[MAX_CHILDREN - 1], 1001, &store));

        let num_children = children.len();
        assert!(num_children > MAX_CHILDREN);

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn interleaved_keys_many_duplicates() {
        // Each key has many duplicates, interleaved in input
        let store = InMemoryStore::default();
        let num_keys = 5;
        let keys = sorted_keys(num_keys);
        let copies_per_key = MAX_CHILDREN / 2;

        // Create interleaved: A, B, C, D, E, A, B, C, D, E, ...
        let mut children = Vec::new();
        for i in 0..copies_per_key {
            for (j, key) in keys.iter().enumerate() {
                children.push(make_leaf(key, (i * 100 + j) as u64, &store));
            }
        }

        let num_children = children.len();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), num_children);
        assert_keys_not_split(&parents, &store);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn minimum_input_to_trigger_main_algorithm() {
        // Exactly MAX_CHILDREN + 1 children (minimum to skip early return)
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 1);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), MAX_CHILDREN + 1);
        assert_eq!(parents.len(), 2);
        assert_max_children_respected(&parents, &store);
    }

    #[test]
    fn maximum_input_for_early_return() {
        // Exactly MAX_CHILDREN children (maximum to use early return)
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN);
        let children: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| make_leaf(k, i as u64, &store))
            .collect();

        let parents = HtreeNode::from_many_children(children, &store).unwrap();

        assert_eq!(total_children(&parents, &store), MAX_CHILDREN);
        assert_eq!(parents.len(), 1);
    }

    // ==================== Subtree preservation tests ====================
    // These tests verify that internal nodes (subtrees) are passed through
    // without being traversed (fetch_children not called).

    /// Creates an internal node (height 1) containing the given leaves.
    fn make_internal_node(leaves: Vec<HtreeNode<u64>>, store: &InMemoryStore) -> HtreeNode<u64> {
        HtreeNode::from_children(leaves, store).unwrap()
    }

    #[test]
    fn subtrees_passed_through_unchanged() {
        // Create internal nodes (subtrees) and pass them to from_many_children.
        // Verify that the subtrees appear in the output with same hkey (not traversed).
        let store = InMemoryStore::default();
        let keys = sorted_keys(6);

        // Create 3 internal nodes, each with 2 leaves
        let subtree1 = make_internal_node(
            vec![
                make_leaf(&keys[0], 0, &store),
                make_leaf(&keys[0], 1, &store),
            ],
            &store,
        );
        let subtree2 = make_internal_node(
            vec![
                make_leaf(&keys[2], 2, &store),
                make_leaf(&keys[2], 3, &store),
            ],
            &store,
        );
        let subtree3 = make_internal_node(
            vec![
                make_leaf(&keys[4], 4, &store),
                make_leaf(&keys[4], 5, &store),
            ],
            &store,
        );

        // Record the hkeys before passing to from_many_children
        let hkey1 = subtree1.hkey.clone();
        let hkey2 = subtree2.hkey.clone();
        let hkey3 = subtree3.hkey.clone();

        let subtrees = vec![subtree1, subtree2, subtree3];
        let parents = HtreeNode::from_many_children(subtrees, &store).unwrap();

        // Should create one parent containing all 3 subtrees
        assert_eq!(parents.len(), 1);
        assert_eq!(parents[0].height, 2); // Parent of height-1 nodes

        // Verify the subtrees are passed through with same hkeys
        let children = parents[0].fetch_children(&store).unwrap();
        assert_eq!(children.len(), 3);

        // The hkeys should be unchanged - subtrees were not reconstructed
        assert!(children.iter().any(|c| c.hkey == hkey1));
        assert!(children.iter().any(|c| c.hkey == hkey2));
        assert!(children.iter().any(|c| c.hkey == hkey3));
    }

    #[test]
    fn subtrees_not_traversed_when_regrouping() {
        // When subtrees need to be split across multiple parents,
        // they should still not be traversed - just moved as opaque nodes.
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 10);

        // Create MAX_CHILDREN + 5 internal nodes
        let mut subtrees = Vec::new();
        let mut original_hkeys = Vec::new();

        for (i, key) in keys.iter().take(MAX_CHILDREN + 5).enumerate() {
            let subtree = make_internal_node(
                vec![
                    make_leaf(key, (i * 10) as u64, &store),
                    make_leaf(key, (i * 10 + 1) as u64, &store),
                ],
                &store,
            );
            original_hkeys.push(subtree.hkey.clone());
            subtrees.push(subtree);
        }

        let parents = HtreeNode::from_many_children(subtrees, &store).unwrap();

        // Should create 2 parents (since > MAX_CHILDREN subtrees)
        assert_eq!(parents.len(), 2);
        assert_eq!(parents[0].height, 2);
        assert_eq!(parents[1].height, 2);

        // Collect all child hkeys from both parents
        let mut result_hkeys = Vec::new();
        for parent in &parents {
            for child in parent.fetch_children(&store).unwrap() {
                result_hkeys.push(child.hkey.clone());
            }
        }

        // All original hkeys should be present - subtrees were not reconstructed
        assert_eq!(result_hkeys.len(), original_hkeys.len());
        for hkey in &original_hkeys {
            assert!(
                result_hkeys.contains(hkey),
                "Subtree hkey not found in output - subtree may have been traversed"
            );
        }
    }

    #[test]
    fn subtrees_preserve_internal_structure() {
        // Verify that the internal structure of subtrees is preserved
        // by checking we can still fetch their children correctly.
        let store = InMemoryStore::default();
        let keys = sorted_keys(4);

        // Create 2 subtrees with known leaf values
        let subtree1 = make_internal_node(
            vec![
                make_leaf(&keys[0], 100, &store),
                make_leaf(&keys[0], 101, &store),
            ],
            &store,
        );
        let subtree2 = make_internal_node(
            vec![
                make_leaf(&keys[2], 200, &store),
                make_leaf(&keys[2], 201, &store),
            ],
            &store,
        );

        let subtrees = vec![subtree1, subtree2];
        let parents = HtreeNode::from_many_children(subtrees, &store).unwrap();

        assert_eq!(parents.len(), 1);

        // Fetch the subtrees from the parent
        let children = parents[0].fetch_children(&store).unwrap();
        assert_eq!(children.len(), 2);

        // Verify we can still traverse into the subtrees and get the original leaves
        let mut all_leaf_keys: Vec<UUID> = Vec::new();
        for subtree in &children {
            assert_eq!(subtree.height, 1); // Subtrees should be height 1
            let leaves = subtree.fetch_children(&store).unwrap();
            for leaf in leaves {
                all_leaf_keys.push(leaf.key);
            }
        }

        // Should have 4 leaves total
        assert_eq!(all_leaf_keys.len(), 4);
    }

    #[test]
    fn many_subtrees_with_same_key_stay_together() {
        // Multiple subtrees with the same key should stay in the same parent
        let store = InMemoryStore::default();
        let keys = sorted_keys(2);

        // Create 5 subtrees, all with keys[0]
        let mut subtrees = Vec::new();
        for i in 0..5 {
            let subtree = make_internal_node(
                vec![
                    make_leaf(&keys[0], (i * 10) as u64, &store),
                    make_leaf(&keys[0], (i * 10 + 1) as u64, &store),
                ],
                &store,
            );
            subtrees.push(subtree);
        }

        // Add 2 subtrees with keys[1]
        for i in 0..2 {
            let subtree = make_internal_node(
                vec![
                    make_leaf(&keys[1], (100 + i * 10) as u64, &store),
                    make_leaf(&keys[1], (100 + i * 10 + 1) as u64, &store),
                ],
                &store,
            );
            subtrees.push(subtree);
        }

        let parents = HtreeNode::from_many_children(subtrees, &store).unwrap();

        // All 7 subtrees fit in one parent
        assert_eq!(parents.len(), 1);

        // Verify keys are not split
        let children = parents[0].fetch_children(&store).unwrap();
        assert_eq!(children.len(), 7);

        // Count subtrees per key
        let key0_count = children.iter().filter(|c| c.key == keys[0]).count();
        let key1_count = children.iter().filter(|c| c.key == keys[1]).count();
        assert_eq!(key0_count, 5);
        assert_eq!(key1_count, 2);
    }

    #[test]
    fn deep_subtrees_passed_through() {
        // Create deeper subtrees (height 2) and verify they pass through
        let store = InMemoryStore::default();
        let keys = sorted_keys(4);

        // Create height-1 nodes
        let internal1 = make_internal_node(
            vec![
                make_leaf(&keys[0], 0, &store),
                make_leaf(&keys[0], 1, &store),
            ],
            &store,
        );
        let internal2 = make_internal_node(
            vec![
                make_leaf(&keys[0], 2, &store),
                make_leaf(&keys[0], 3, &store),
            ],
            &store,
        );

        // Create height-2 node containing the height-1 nodes
        let deep_subtree = HtreeNode::from_children(vec![internal1, internal2], &store).unwrap();
        assert_eq!(deep_subtree.height, 2);

        // Create another simple subtree
        let simple_subtree = make_internal_node(
            vec![
                make_leaf(&keys[2], 10, &store),
                make_leaf(&keys[2], 11, &store),
            ],
            &store,
        );

        // This should fail because heights are inconsistent
        let result =
            HtreeNode::<u64>::from_many_children(vec![deep_subtree, simple_subtree], &store);

        // Heights are inconsistent (2 vs 1), so this should error
        assert!(result.is_err());
    }

    #[test]
    fn subtrees_same_height_different_depths() {
        // All input subtrees must have the same height
        // This test verifies correct behavior with uniform height-1 subtrees
        let store = InMemoryStore::default();
        let keys = sorted_keys(MAX_CHILDREN + 5);

        // Create many height-1 subtrees with varying number of leaves (but same height)
        let mut subtrees = Vec::new();
        for (i, key) in keys.iter().take(MAX_CHILDREN + 5).enumerate() {
            // Some subtrees have 2 leaves, some have 3
            let num_leaves = if i % 2 == 0 { 2 } else { 3 };
            let leaves: Vec<_> = (0..num_leaves)
                .map(|j| make_leaf(key, (i * 100 + j) as u64, &store))
                .collect();
            let subtree = make_internal_node(leaves, &store);
            assert_eq!(subtree.height, 1);
            subtrees.push(subtree);
        }

        let parents = HtreeNode::from_many_children(subtrees, &store).unwrap();

        // Should succeed and create 2 parents
        assert_eq!(parents.len(), 2);

        // All parents should have height 2
        for parent in &parents {
            assert_eq!(parent.height, 2);
        }
    }
}
