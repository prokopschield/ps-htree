use ps_hkey::Store;

use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Splits the tree at the first leaf that matches the predicate.
    ///
    /// Assumes the leaves are sorted such that the predicate is monotonic:
    /// all non-matching elements come first, followed by all matching elements.
    /// The predicate receives an [`HtreeNode`] and should examine only the `key`
    /// field to maintain consistency between leaf and internal node routing.
    ///
    /// Returns `(left, right)` where `left` contains all non-matching leaves
    /// and `right` contains all matching leaves (starting from the first match).
    ///
    /// # Arguments
    ///
    /// * `predicate` - A monotonic predicate over the sorted key order.
    /// * `store` - Persistence backend.
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeSplitError::CorruptedState`] if internal node state is invalid.
    /// - [`HtreeNodeSplitError::FromChildren`] if node reconstruction fails.
    /// - [`HtreeNodeSplitError::Store`] if store operations fail.
    /// - [`HtreeNodeSplitError::UnpackChildren`] if child deserialization fails.
    pub fn split<S, F>(
        &self,
        predicate: &F,
        store: &S,
    ) -> Result<(Option<Self>, Option<Self>), HtreeNodeSplitError<S>>
    where
        S: Store,
        F: Fn(&Self) -> bool,
    {
        if self.is_empty() {
            return Ok((None, None));
        }

        // Descend: at each level, split children into left siblings, transition
        // child, and right siblings. Push siblings onto the stack, continue
        // descending into the transition child.
        let mut stack: Vec<(Vec<Self>, Vec<Self>)> = Vec::new();
        let mut current = self.clone();

        let (mut left, mut right) = loop {
            if current.is_leaf() {
                break if predicate(&current) {
                    (None, Some(current))
                } else {
                    (Some(current), None)
                };
            }

            let children = current.fetch_children(store)?;
            let split_idx = children.partition_point(|c| !predicate(c));

            if split_idx == 0 {
                // The first child's min key matches, so by monotonicity all leaves match.
                break (None, Some(current));
            }

            let recurse_idx = split_idx - 1;
            let mut iter = children.into_iter();
            let left_siblings: Vec<Self> = iter.by_ref().take(recurse_idx).collect();
            let Some(transition) = iter.next() else {
                // Unreachable: split_idx >= 1 guarantees at least one element remains
                // after consuming recurse_idx items.
                return Err(HtreeNodeSplitError::CorruptedState);
            };
            let right_siblings: Vec<Self> = iter.collect();

            stack.push((left_siblings, right_siblings));
            current = transition;
        };

        // Ascend: rebuild from bottom to top, wrapping each level's siblings
        // around the accumulated left/right halves.
        while let Some((left_siblings, right_siblings)) = stack.pop() {
            let mut left_children = left_siblings;
            if let Some(l) = left {
                left_children.push(l);
            }

            let mut right_children = Vec::new();
            if let Some(r) = right {
                right_children.push(r);
            }
            right_children.extend(right_siblings);

            left = if left_children.is_empty() {
                None
            } else {
                Some(Self::from_children(left_children, store)?)
            };

            right = if right_children.is_empty() {
                None
            } else {
                Some(Self::from_children(right_children, store)?)
            };
        }

        Ok((left, right))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeSplitError<S: Store> {
    #[error("HtreeNode's state is corrupted.")]
    CorruptedState,

    #[error("Node reconstruction failed.")]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),

    #[error("Store error: {0}")]
    Store(S::Error),

    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodeSplitError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedState,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodeSplitError<S> {
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    /// Collects all keys from a tree in sorted order.
    fn collect_keys(
        tree: &HtreeNode<u64>,
        store: &InMemoryStore,
    ) -> Result<Vec<UUID>, Box<dyn std::error::Error>> {
        Ok(tree.iter_keys(store).collect::<Result<Vec<_>, _>>()?)
    }

    /// Builds a single-root tree from the given keys (with dummy u64 values).
    fn make_tree(
        keys: &[UUID],
        store: &InMemoryStore,
    ) -> Result<HtreeNode<u64>, Box<dyn std::error::Error>> {
        if keys.is_empty() {
            return Ok(HtreeNode::default());
        }

        let leaves = keys
            .iter()
            .enumerate()
            .map(|(i, k)| HtreeNode::<u64>::from_kvp(k, &(i as u64), store))
            .collect::<Result<Vec<_>, _>>()?;

        let mut nodes = HtreeNode::from_many_children(leaves, store)?;
        while nodes.len() > 1 {
            nodes = HtreeNode::from_many_children(nodes, store)?;
        }
        nodes
            .into_iter()
            .next()
            .ok_or("expected at least one node".into())
    }

    /// Generates `n` random sorted UUIDs.
    fn gen_keys(n: usize) -> Vec<UUID> {
        let mut keys: Vec<UUID> = (0..n).map(|_| UUID::gen_v4()).collect();
        keys.sort();
        keys
    }

    // ── empty tree ──────────────────────────────────────────────

    #[test]
    fn empty_tree_returns_none_none() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let tree: HtreeNode<()> = HtreeNode::default();

        let (left, right) = tree.split(&|_| true, &store)?;

        assert!(left.is_none());
        assert!(right.is_none());
        Ok(())
    }

    #[test]
    fn empty_tree_with_false_predicate() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let tree: HtreeNode<()> = HtreeNode::default();

        let (left, right) = tree.split(&|_| false, &store)?;

        assert!(left.is_none());
        assert!(right.is_none());
        Ok(())
    }

    // ── single leaf ─────────────────────────────────────────────

    #[test]
    fn leaf_matches_goes_to_right() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let leaf = HtreeNode::<u64>::from_kvp(&key, &1, &store)?;
        let (left, right) = leaf.split(&|_| true, &store)?;

        assert!(left.is_none());
        let right = right.ok_or("expected right")?;
        assert_eq!(right.key, key);
        Ok(())
    }

    #[test]
    fn leaf_does_not_match_goes_to_left() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let leaf = HtreeNode::<u64>::from_kvp(&key, &1, &store)?;
        let (left, right) = leaf.split(&|_| false, &store)?;

        let left = left.ok_or("expected left")?;
        assert_eq!(left.key, key);
        assert!(right.is_none());
        Ok(())
    }

    #[test]
    fn leaf_equal_to_threshold_goes_to_right() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let leaf = HtreeNode::<u64>::from_kvp(&key, &1, &store)?;
        let (left, right) = leaf.split(&|node| node.key >= key, &store)?;

        assert!(left.is_none());
        let right = right.ok_or("expected right")?;
        assert_eq!(right.key, key);
        Ok(())
    }

    // ── two leaves ──────────────────────────────────────────────

    #[test]
    fn two_leaves_split_between() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(2);
        let tree = make_tree(&keys, &store)?;

        let (left, right) = tree.split(&|node| node.key >= keys[1], &store)?;

        let left_keys = collect_keys(&left.ok_or("expected left")?, &store)?;
        let right_keys = collect_keys(&right.ok_or("expected right")?, &store)?;

        assert_eq!(left_keys, vec![keys[0]]);
        assert_eq!(right_keys, vec![keys[1]]);
        Ok(())
    }

    // ── all keys on one side ────────────────────────────────────

    #[test]
    fn all_match_returns_none_some() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(5);
        let tree = make_tree(&keys, &store)?;

        // Predicate matches the smallest key → all keys match
        let (left, right) = tree.split(&|node| node.key >= keys[0], &store)?;

        assert!(left.is_none());
        let right_keys = collect_keys(&right.ok_or("expected right")?, &store)?;
        assert_eq!(right_keys, keys);
        Ok(())
    }

    #[test]
    fn none_match_returns_some_none() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let all = gen_keys(6);
        let tree_keys = &all[..5];
        let threshold = all[5]; // strictly larger than all tree keys
        let tree = make_tree(tree_keys, &store)?;

        let (left, right) = tree.split(&|node| node.key >= threshold, &store)?;

        assert!(right.is_none());
        let left_keys = collect_keys(&left.ok_or("expected left")?, &store)?;
        assert_eq!(left_keys, tree_keys);
        Ok(())
    }

    #[test]
    fn predicate_gte_nil_puts_everything_right() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(5);
        let tree = make_tree(&keys, &store)?;

        let (left, right) = tree.split(&|node| node.key >= UUID::nil(), &store)?;

        assert!(left.is_none());
        let right_keys = collect_keys(&right.ok_or("expected right")?, &store)?;
        assert_eq!(right_keys, keys);
        Ok(())
    }

    // ── partition property ──────────────────────────────────────

    #[test]
    fn left_keys_do_not_match_predicate() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let threshold = keys[5];

        let (left, _right) = tree.split(&|node| node.key >= threshold, &store)?;

        if let Some(left) = left {
            for k in collect_keys(&left, &store)? {
                assert!(k < threshold, "{k:?} should be < {threshold:?}");
            }
        }
        Ok(())
    }

    #[test]
    fn right_keys_match_predicate() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let threshold = keys[5];

        let (_left, right) = tree.split(&|node| node.key >= threshold, &store)?;

        if let Some(right) = right {
            for k in collect_keys(&right, &store)? {
                assert!(k >= threshold, "{k:?} should be >= {threshold:?}");
            }
        }
        Ok(())
    }

    // ── key conservation ────────────────────────────────────────

    #[test]
    fn split_preserves_all_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let threshold = keys[5];

        let (left, right) = tree.split(&|node| node.key >= threshold, &store)?;

        let mut all_keys = Vec::new();
        if let Some(left) = &left {
            all_keys.extend(collect_keys(left, &store)?);
        }
        if let Some(right) = &right {
            all_keys.extend(collect_keys(right, &store)?);
        }
        all_keys.sort();

        assert_eq!(all_keys, keys);
        Ok(())
    }

    #[test]
    fn split_counts_match() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let threshold = keys[3];

        let (left, right) = tree.split(&|node| node.key >= threshold, &store)?;

        let left_count = match &left {
            Some(left) => collect_keys(left, &store)?.len(),
            None => 0,
        };
        let right_count = match &right {
            Some(right) => collect_keys(right, &store)?.len(),
            None => 0,
        };

        assert_eq!(left_count, 3);
        assert_eq!(right_count, 7);
        assert_eq!(left_count + right_count, keys.len());
        Ok(())
    }

    // ── find_one on split halves ────────────────────────────────

    #[test]
    fn find_one_works_on_split_halves() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let threshold = keys[5];

        let (left, right) = tree.split(&|node| node.key >= threshold, &store)?;
        let left = left.ok_or("expected left")?;
        let right = right.ok_or("expected right")?;

        for &k in &keys[..5] {
            assert!(left.find_one(&k, &store)?.is_some());
            assert!(right.find_one(&k, &store)?.is_none());
        }
        for &k in &keys[5..] {
            assert!(left.find_one(&k, &store)?.is_none());
            assert!(right.find_one(&k, &store)?.is_some());
        }
        Ok(())
    }

    // ── structural validity of halves ───────────────────────────

    #[test]
    fn both_halves_are_valid_trees() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let threshold = keys[5];

        let (left, right) = tree.split(&|node| node.key >= threshold, &store)?;
        let left = left.ok_or("expected left")?;
        let right = right.ok_or("expected right")?;

        assert!(!left.is_empty());
        assert!(!right.is_empty());

        let left_first = left.first(&store)?.ok_or("expected first in left")?;
        let left_last = left.last(&store)?.ok_or("expected last in left")?;
        assert!(left_first.key < threshold);
        assert!(left_last.key < threshold);
        assert!(left_first.key <= left_last.key);

        let right_first = right.first(&store)?.ok_or("expected first in right")?;
        let right_last = right.last(&store)?.ok_or("expected last in right")?;
        assert!(right_first.key >= threshold);
        assert!(right_last.key >= threshold);
        assert!(right_first.key <= right_last.key);
        Ok(())
    }

    // ── immutability ────────────────────────────────────────────

    #[test]
    fn split_does_not_mutate_original() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(5);
        let tree = make_tree(&keys, &store)?;
        let original_hkey = tree.hkey.clone();

        let _result = tree.split(&|node| node.key >= keys[2], &store)?;

        assert_eq!(tree.hkey, original_hkey);
        let original_keys = collect_keys(&tree, &store)?;
        assert_eq!(original_keys, keys);
        Ok(())
    }

    // ── exhaustive split at every position ──────────────────────

    #[test]
    fn split_at_every_key_preserves_and_partitions() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(8);
        let tree = make_tree(&keys, &store)?;

        for &threshold in &keys {
            let (left, right) = tree.split(&|node| node.key >= threshold, &store)?;

            let mut all_keys = Vec::new();
            if let Some(left) = &left {
                for k in collect_keys(left, &store)? {
                    assert!(k < threshold, "{k:?} should be < {threshold:?}");
                    all_keys.push(k);
                }
            }
            if let Some(right) = &right {
                for k in collect_keys(right, &store)? {
                    assert!(k >= threshold, "{k:?} should be >= {threshold:?}");
                    all_keys.push(k);
                }
            }
            all_keys.sort();
            assert_eq!(all_keys, keys, "split at {threshold:?} lost keys");
        }
        Ok(())
    }

    // ── re-split ────────────────────────────────────────────────

    #[test]
    fn resplit_preserves_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        let (left, right) = tree.split(&|node| node.key >= keys[5], &store)?;
        let left = left.ok_or("expected left")?;
        let right = right.ok_or("expected right")?;

        let (ll, lr) = left.split(&|node| node.key >= keys[2], &store)?;

        let mut all_keys = Vec::new();
        if let Some(n) = &ll {
            all_keys.extend(collect_keys(n, &store)?);
        }
        if let Some(n) = &lr {
            all_keys.extend(collect_keys(n, &store)?);
        }
        all_keys.extend(collect_keys(&right, &store)?);
        all_keys.sort();

        assert_eq!(all_keys, keys);
        Ok(())
    }

    // ── large tree ──────────────────────────────────────────────

    #[test]
    fn large_tree_split_preserves_all_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(50);
        let tree = make_tree(&keys, &store)?;
        let threshold = keys[25];

        let (left, right) = tree.split(&|node| node.key >= threshold, &store)?;

        let mut all_keys = Vec::new();
        if let Some(left) = &left {
            all_keys.extend(collect_keys(left, &store)?);
        }
        if let Some(right) = &right {
            all_keys.extend(collect_keys(right, &store)?);
        }
        all_keys.sort();

        assert_eq!(all_keys, keys);
        Ok(())
    }

    #[test]
    fn large_tree_partition_property() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(50);
        let tree = make_tree(&keys, &store)?;
        let threshold = keys[25];

        let (left, right) = tree.split(&|node| node.key >= threshold, &store)?;

        if let Some(left) = &left {
            for k in collect_keys(left, &store)? {
                assert!(k < threshold);
            }
        }
        if let Some(right) = &right {
            for k in collect_keys(right, &store)? {
                assert!(k >= threshold);
            }
        }
        Ok(())
    }

    // ── consistency with split_at ───────────────────────────────

    #[test]
    fn consistent_with_split_at() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        for &threshold in &keys {
            let (split_left, split_right) = tree.split(&|node| node.key >= threshold, &store)?;
            let (at_left, at_right) = tree.split_at(&threshold, &store)?;

            let split_left_keys = match &split_left {
                Some(n) => collect_keys(n, &store)?,
                None => vec![],
            };
            let split_right_keys = match &split_right {
                Some(n) => collect_keys(n, &store)?,
                None => vec![],
            };
            let at_left_keys = match &at_left {
                Some(n) => collect_keys(n, &store)?,
                None => vec![],
            };
            let at_right_keys = match &at_right {
                Some(n) => collect_keys(n, &store)?,
                None => vec![],
            };

            assert_eq!(
                split_left_keys, at_left_keys,
                "left keys differ at threshold {threshold:?}"
            );
            assert_eq!(
                split_right_keys, at_right_keys,
                "right keys differ at threshold {threshold:?}"
            );
        }
        Ok(())
    }
}
