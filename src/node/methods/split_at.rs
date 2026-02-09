use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode};

impl<T> HtreeNode<T> {
    /// Splits the tree into two halves at the given key.
    ///
    /// Returns a `(lesser, greater)` pair where:
    /// - `lesser` contains all leaves with keys strictly less than `key`
    /// - `greater` contains all leaves with keys greater than or equal to `key`
    ///
    /// Either half is `None` when it would contain no leaves.
    /// An empty tree always returns `(None, None)`.
    ///
    /// This is a convenience wrapper around [`split`](Self::split) with the
    /// predicate `|node| node.key < key`.
    ///
    /// # Arguments
    ///
    /// * `key` - The split point. Must implement [`HtreeKey`] for UUID conversion via the store.
    /// * `store` - The persistence backend for key conversion and child node resolution.
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeSplitAtError::Key`] if key conversion to a UUID fails.
    /// - [`HtreeNodeSplitAtError::Split`] if the underlying split operation fails.
    /// - [`HtreeNodeSplitAtError::Store`] if any store operation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ps_hkey::InMemoryStore;
    /// use ps_uuid::UUID;
    /// use ps_htree::HtreeNode;
    ///
    /// let store = InMemoryStore::default();
    ///
    /// // Empty tree returns (None, None)
    /// let empty: HtreeNode<()> = HtreeNode::default();
    /// let (lt, gte) = empty.split_at(&UUID::nil(), &store).unwrap();
    /// assert!(lt.is_none());
    /// assert!(gte.is_none());
    ///
    /// // Single leaf: key == split point goes to greater
    /// let key = UUID::gen_v4();
    /// let leaf = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
    /// let (lt, gte) = leaf.split_at(&key, &store).unwrap();
    /// assert!(lt.is_none());
    /// assert!(gte.is_some());
    /// ```
    pub fn split_at<S: Store>(
        &self,
        key: &impl HtreeKey,
        store: &S,
    ) -> Result<(Option<Self>, Option<Self>), HtreeNodeSplitAtError<S>> {
        let key = key.try_to_uuid(store)?;

        Ok(self.split(&|node| node.key < key, store)?)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeSplitAtError<S: Store> {
    #[error("Key conversion error: {0}")]
    Key(crate::HtreeKeyError<S>),
    #[error(transparent)]
    Split(crate::HtreeNodeSplitError<S>),
    #[error("Store error: {0}")]
    Store(S::Error),
}

#[allow(clippy::match_wildcard_for_single_variants)]
impl<S: Store> From<crate::HtreeKeyError<S>> for HtreeNodeSplitAtError<S> {
    fn from(value: crate::HtreeKeyError<S>) -> Self {
        match value {
            crate::HtreeKeyError::Store(err) => Self::Store(err),
            err => Self::Key(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeSplitError<S>> for HtreeNodeSplitAtError<S> {
    fn from(value: crate::HtreeNodeSplitError<S>) -> Self {
        match value {
            crate::HtreeNodeSplitError::Store(err) => Self::Store(err),
            err => Self::Split(err),
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

    /// Builds a tree from the given keys (with dummy u64 values).
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
        let key = UUID::gen_v4();

        let (lt, gte) = tree.split_at(&key, &store)?;

        assert!(lt.is_none());
        assert!(gte.is_none());
        Ok(())
    }

    #[test]
    fn empty_tree_split_at_nil() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let tree: HtreeNode<()> = HtreeNode::default();

        let (lt, gte) = tree.split_at(&UUID::nil(), &store)?;

        assert!(lt.is_none());
        assert!(gte.is_none());
        Ok(())
    }

    // ── single leaf ─────────────────────────────────────────────

    #[test]
    fn leaf_less_than_key_goes_to_lesser() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(2);

        let leaf = HtreeNode::<u64>::from_kvp(&keys[0], &1, &store)?;
        let (lt, gte) = leaf.split_at(&keys[1], &store)?;

        let lt = lt.ok_or("expected lesser")?;
        assert_eq!(lt.key, keys[0]);
        assert!(gte.is_none());
        Ok(())
    }

    #[test]
    fn leaf_equal_to_key_goes_to_greater() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let leaf = HtreeNode::<u64>::from_kvp(&key, &1, &store)?;
        let (lt, gte) = leaf.split_at(&key, &store)?;

        assert!(lt.is_none());
        let gte = gte.ok_or("expected greater")?;
        assert_eq!(gte.key, key);
        Ok(())
    }

    #[test]
    fn leaf_greater_than_key_goes_to_greater() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(2);

        let leaf = HtreeNode::<u64>::from_kvp(&keys[1], &1, &store)?;
        let (lt, gte) = leaf.split_at(&keys[0], &store)?;

        assert!(lt.is_none());
        let gte = gte.ok_or("expected greater")?;
        assert_eq!(gte.key, keys[1]);
        Ok(())
    }

    // ── two leaves ──────────────────────────────────────────────

    #[test]
    fn two_leaves_split_between() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(2);
        let tree = make_tree(&keys, &store)?;

        // Split at the larger key: lesser gets keys[0], greater gets keys[1]
        let (lt, gte) = tree.split_at(&keys[1], &store)?;

        let lt_keys = collect_keys(&lt.ok_or("expected lesser")?, &store)?;
        let gte_keys = collect_keys(&gte.ok_or("expected greater")?, &store)?;

        assert_eq!(lt_keys, vec![keys[0]]);
        assert_eq!(gte_keys, vec![keys[1]]);
        Ok(())
    }

    // ── all keys on one side ────────────────────────────────────

    #[test]
    fn all_keys_less_than_split() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let all = gen_keys(6);
        let tree_keys = &all[..5];
        let split_key = all[5]; // strictly larger than all tree keys
        let tree = make_tree(tree_keys, &store)?;

        let (lt, gte) = tree.split_at(&split_key, &store)?;

        assert!(gte.is_none());
        let lt_keys = collect_keys(&lt.ok_or("expected lesser")?, &store)?;
        assert_eq!(lt_keys, tree_keys);
        Ok(())
    }

    #[test]
    fn all_keys_gte_split() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(5);
        let tree = make_tree(&keys, &store)?;

        // Split at the smallest key in the tree: all keys >= it
        let (lt, gte) = tree.split_at(&keys[0], &store)?;

        assert!(lt.is_none());
        let gte_keys = collect_keys(&gte.ok_or("expected greater")?, &store)?;
        assert_eq!(gte_keys, keys);
        Ok(())
    }

    #[test]
    fn split_at_nil_puts_everything_in_greater() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(5);
        let tree = make_tree(&keys, &store)?;

        let (lt, gte) = tree.split_at(&UUID::nil(), &store)?;

        assert!(lt.is_none());
        let gte_keys = collect_keys(&gte.ok_or("expected greater")?, &store)?;
        assert_eq!(gte_keys, keys);
        Ok(())
    }

    // ── partition property ──────────────────────────────────────

    #[test]
    fn lesser_keys_are_strictly_less() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let split_key = keys[5];

        let (lt, _gte) = tree.split_at(&split_key, &store)?;

        if let Some(lt) = lt {
            for key in collect_keys(&lt, &store)? {
                assert!(key < split_key, "{key:?} should be < {split_key:?}");
            }
        }
        Ok(())
    }

    #[test]
    fn greater_keys_are_gte() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let split_key = keys[5];

        let (_lt, gte) = tree.split_at(&split_key, &store)?;

        if let Some(gte) = gte {
            for key in collect_keys(&gte, &store)? {
                assert!(key >= split_key, "{key:?} should be >= {split_key:?}");
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
        let split_key = keys[5];

        let (lt, gte) = tree.split_at(&split_key, &store)?;

        let mut all_keys = Vec::new();
        if let Some(lt) = &lt {
            all_keys.extend(collect_keys(lt, &store)?);
        }
        if let Some(gte) = &gte {
            all_keys.extend(collect_keys(gte, &store)?);
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

        // Split at keys[3]: 3 keys in lesser, 7 in greater
        let split_key = keys[3];
        let (lt, gte) = tree.split_at(&split_key, &store)?;

        let lt_count = match &lt {
            Some(lt) => collect_keys(lt, &store)?.len(),
            None => 0,
        };
        let gte_count = match &gte {
            Some(gte) => collect_keys(gte, &store)?.len(),
            None => 0,
        };

        assert_eq!(lt_count, 3);
        assert_eq!(gte_count, 7);
        assert_eq!(lt_count + gte_count, keys.len());
        Ok(())
    }

    // ── existing vs nonexistent keys ────────────────────────────

    #[test]
    fn split_at_existing_key_puts_it_in_greater() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(5);
        let tree = make_tree(&keys, &store)?;
        let split_key = keys[2];

        let (lt, gte) = tree.split_at(&split_key, &store)?;

        let gte_keys = collect_keys(&gte.ok_or("expected greater")?, &store)?;
        assert!(gte_keys.contains(&split_key));

        if let Some(lt) = lt {
            let lt_keys = collect_keys(&lt, &store)?;
            assert!(!lt_keys.contains(&split_key));
        }
        Ok(())
    }

    #[test]
    fn split_at_nonexistent_key() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(5);
        let tree = make_tree(&keys, &store)?;

        // A fresh random key is (almost certainly) not in the tree
        let split_key = UUID::gen_v4();
        let (lt, gte) = tree.split_at(&split_key, &store)?;

        let mut all_keys = Vec::new();
        if let Some(lt) = &lt {
            for k in collect_keys(lt, &store)? {
                assert!(k < split_key);
                all_keys.push(k);
            }
        }
        if let Some(gte) = &gte {
            for k in collect_keys(gte, &store)? {
                assert!(k >= split_key);
                all_keys.push(k);
            }
        }
        all_keys.sort();
        assert_eq!(all_keys, keys);
        Ok(())
    }

    // ── find_one on split halves ────────────────────────────────

    #[test]
    fn find_one_works_on_split_halves() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let split_key = keys[5];

        let (lt, gte) = tree.split_at(&split_key, &store)?;
        let lt = lt.ok_or("expected lesser")?;
        let gte = gte.ok_or("expected greater")?;

        // Keys < split_key should be findable only in lesser
        for &k in &keys[..5] {
            assert!(lt.find_one(&k, &store)?.is_some());
            assert!(gte.find_one(&k, &store)?.is_none());
        }
        // Keys >= split_key should be findable only in greater
        for &k in &keys[5..] {
            assert!(lt.find_one(&k, &store)?.is_none());
            assert!(gte.find_one(&k, &store)?.is_some());
        }
        Ok(())
    }

    // ── structural validity of halves ───────────────────────────

    #[test]
    fn both_halves_are_valid_trees() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;
        let split_key = keys[5];

        let (lt, gte) = tree.split_at(&split_key, &store)?;
        let lt = lt.ok_or("expected lesser")?;
        let gte = gte.ok_or("expected greater")?;

        assert!(!lt.is_empty());
        assert!(!gte.is_empty());

        // first/last should work on both halves
        let lt_first = lt.first(&store)?.ok_or("expected first in lesser")?;
        let lt_last = lt.last(&store)?.ok_or("expected last in lesser")?;
        assert!(lt_first.key < split_key);
        assert!(lt_last.key < split_key);
        assert!(lt_first.key <= lt_last.key);

        let gte_first = gte.first(&store)?.ok_or("expected first in greater")?;
        let gte_last = gte.last(&store)?.ok_or("expected last in greater")?;
        assert!(gte_first.key >= split_key);
        assert!(gte_last.key >= split_key);
        assert!(gte_first.key <= gte_last.key);
        Ok(())
    }

    // ── immutability ────────────────────────────────────────────

    #[test]
    fn split_does_not_mutate_original() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(5);
        let tree = make_tree(&keys, &store)?;
        let original_hkey = tree.hkey.clone();

        let _result = tree.split_at(&keys[2], &store)?;

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

        for &split_key in &keys {
            let (lt, gte) = tree.split_at(&split_key, &store)?;

            let mut all_keys = Vec::new();
            if let Some(lt) = &lt {
                for k in collect_keys(lt, &store)? {
                    assert!(k < split_key, "{k:?} should be < {split_key:?}");
                    all_keys.push(k);
                }
            }
            if let Some(gte) = &gte {
                for k in collect_keys(gte, &store)? {
                    assert!(k >= split_key, "{k:?} should be >= {split_key:?}");
                    all_keys.push(k);
                }
            }
            all_keys.sort();
            assert_eq!(all_keys, keys, "split at {split_key:?} lost keys");
        }
        Ok(())
    }

    // ── re-split ────────────────────────────────────────────────

    #[test]
    fn resplit_preserves_keys() -> Result<(), Box<dyn std::error::Error>> {
        let store = InMemoryStore::default();
        let keys = gen_keys(10);
        let tree = make_tree(&keys, &store)?;

        // First split
        let (lt, gte) = tree.split_at(&keys[5], &store)?;
        let lt = lt.ok_or("expected lesser")?;
        let gte = gte.ok_or("expected greater")?;

        // Re-split the lesser half
        let (lt_lt, lt_gte) = lt.split_at(&keys[2], &store)?;

        let mut all_keys = Vec::new();
        if let Some(n) = &lt_lt {
            all_keys.extend(collect_keys(n, &store)?);
        }
        if let Some(n) = &lt_gte {
            all_keys.extend(collect_keys(n, &store)?);
        }
        all_keys.extend(collect_keys(&gte, &store)?);
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
        let split_key = keys[25];

        let (lt, gte) = tree.split_at(&split_key, &store)?;

        let mut all_keys = Vec::new();
        if let Some(lt) = &lt {
            all_keys.extend(collect_keys(lt, &store)?);
        }
        if let Some(gte) = &gte {
            all_keys.extend(collect_keys(gte, &store)?);
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
        let split_key = keys[25];

        let (lt, gte) = tree.split_at(&split_key, &store)?;

        if let Some(lt) = &lt {
            for k in collect_keys(lt, &store)? {
                assert!(k < split_key);
            }
        }
        if let Some(gte) = &gte {
            for k in collect_keys(gte, &store)? {
                assert!(k >= split_key);
            }
        }
        Ok(())
    }
}
