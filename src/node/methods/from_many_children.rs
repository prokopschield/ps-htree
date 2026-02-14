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
}
