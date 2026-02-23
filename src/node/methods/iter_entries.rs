use ps_hkey::Store;
use ps_uuid::UUID;

use crate::{HtreeNode, HtreeValue};

use super::HtreeNodeIterLeavesError;

impl<T: HtreeValue> HtreeNode<T> {
    /// Returns an iterator over all (key, value) pairs in this tree.
    ///
    /// Entries are yielded in sorted key order (smallest to largest).
    /// This combines [`iter_leaves`](Self::iter_leaves) with value unpacking.
    ///
    /// # Arguments
    ///
    /// * `store` - The persistence layer providing child node resolution.
    ///
    /// # Errors
    ///
    /// Each item in the iterator may return:
    /// - [`HtreeNodeIterEntriesError::CorruptedState`] if node state is internally corrupted.
    /// - [`HtreeNodeIterEntriesError::Store`] if store operations fail.
    /// - [`HtreeNodeIterEntriesError::Unpack`] if value deserialization fails.
    /// - [`HtreeNodeIterEntriesError::UnpackChildren`] if unpacking child nodes fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ps_htree::HtreeNode;
    /// use ps_hkey::InMemoryStore;
    /// use ps_uuid::UUID;
    ///
    /// let store = InMemoryStore::default();
    /// let key1 = UUID::gen_v4();
    /// let key2 = UUID::gen_v4();
    ///
    /// let leaf1 = HtreeNode::<u64>::from_kvp(&key1, &1, &store).unwrap();
    /// let leaf2 = HtreeNode::<u64>::from_kvp(&key2, &2, &store).unwrap();
    ///
    /// let tree = HtreeNode::from_many_children([leaf1, leaf2], &store)
    ///     .unwrap()
    ///     .into_iter()
    ///     .next()
    ///     .unwrap();
    ///
    /// let entries: Vec<(UUID, u64)> = tree
    ///     .iter_entries(&store)
    ///     .map(|r| r.unwrap())
    ///     .collect();
    ///
    /// assert_eq!(entries.len(), 2);
    /// ```
    pub fn iter_entries<'a, S: Store>(
        &'a self,
        store: &'a S,
    ) -> impl Iterator<Item = Result<(UUID, T), HtreeNodeIterEntriesError<T, S>>> + 'a {
        self.iter_leaves(store).map(move |res| {
            let leaf = res?;
            let key = leaf.key;

            let bytes = leaf
                .hkey
                .resolve(store)
                .map_err(HtreeNodeIterEntriesError::Store)?;

            let value = T::unpack_from_bytes(bytes, store)?;

            Ok((key, value))
        })
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeIterEntriesError<T: HtreeValue, S: Store> {
    #[error("HtreeNode's state is internally corrupted.")]
    CorruptedState,

    #[error("Store error: {0}")]
    Store(S::Error),

    #[error("Unpack error: {0}")]
    Unpack(T::UnpackError),

    #[error(transparent)]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<T: HtreeValue, S: Store> From<HtreeNodeIterLeavesError<S>>
    for HtreeNodeIterEntriesError<T, S>
{
    fn from(value: HtreeNodeIterLeavesError<S>) -> Self {
        match value {
            HtreeNodeIterLeavesError::Store(err) => Self::Store(err),
            HtreeNodeIterLeavesError::CorruptedState => Self::CorruptedState,
            HtreeNodeIterLeavesError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<T: HtreeValue, S: Store> From<crate::HtreeValueUnpackError<T, S>>
    for HtreeNodeIterEntriesError<T, S>
{
    fn from(value: crate::HtreeValueUnpackError<T, S>) -> Self {
        match value {
            crate::HtreeValueUnpackError::Store(err) => Self::Store(err),
            crate::HtreeValueUnpackError::Unpack(err) => Self::Unpack(err),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn empty_tree_yields_no_entries() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();

        assert!(tree.iter_entries(&store).next().is_none());
    }

    #[test]
    fn single_leaf_yields_one_entry() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let value = 42_u64;

        let tree =
            HtreeNode::from_kvp(&key, &value, &store).expect("from_kvp should create a leaf node");

        let entries: Vec<_> = tree
            .iter_entries(&store)
            .map(|r| r.expect("iter_entries should not fail"))
            .collect();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], (key, 42));
    }

    #[test]
    fn multi_leaf_tree_yields_all_entries() {
        let store = InMemoryStore::default();

        let mut keys: Vec<UUID> = (0..5).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                HtreeNode::<u64>::from_kvp(k, &(i as u64), &store)
                    .expect("from_kvp should create leaf node")
            })
            .collect();

        let tree = HtreeNode::from_many_children(leaves, &store)
            .expect("from_many_children should build tree")
            .into_iter()
            .next()
            .expect("should return at least one root node");

        let entries: Vec<_> = tree
            .iter_entries(&store)
            .map(|r| r.expect("iter_entries should not fail"))
            .collect();

        assert_eq!(entries.len(), 5);

        // Verify keys are in sorted order
        for i in 0..entries.len() - 1 {
            assert!(entries[i].0 < entries[i + 1].0);
        }

        // Verify all keys and values are present
        for (i, key) in keys.iter().enumerate() {
            let entry = entries.iter().find(|(k, _)| k == key);
            assert!(entry.is_some(), "key {key:?} not found");
            assert_eq!(entry.expect("entry should exist").1, i as u64);
        }
    }

    #[test]
    fn entries_are_yielded_in_sorted_order() {
        let store = InMemoryStore::default();

        let keys: Vec<UUID> = (0..20).map(|_| UUID::gen_v4()).collect();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                HtreeNode::<u64>::from_kvp(k, &(i as u64), &store)
                    .expect("from_kvp should create leaf node")
            })
            .collect();

        let tree = HtreeNode::from_many_children(leaves, &store)
            .expect("from_many_children should build tree")
            .into_iter()
            .next()
            .expect("should return at least one root node");

        let entries: Vec<_> = tree
            .iter_entries(&store)
            .map(|r| r.expect("iter_entries should not fail"))
            .collect();

        // Verify entries are sorted by key
        let sorted: Vec<_> = {
            let mut v = entries.clone();
            v.sort_by_key(|(k, _)| *k);
            v
        };

        assert_eq!(entries, sorted);
    }

    #[test]
    fn entries_after_deletion() {
        let store = InMemoryStore::default();

        let mut keys: Vec<UUID> = (0..5).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| {
                HtreeNode::<u64>::from_kvp(k, &(i as u64), &store)
                    .expect("from_kvp should create leaf node")
            })
            .collect();

        let tree = HtreeNode::from_many_children(leaves, &store)
            .expect("from_many_children should build tree")
            .into_iter()
            .next()
            .expect("should return at least one root node");

        let tree = tree
            .delete_one(&keys[2], &store)
            .expect("delete_one should succeed");

        let entries: Vec<_> = tree
            .iter_entries(&store)
            .map(|r| r.expect("iter_entries should not fail"))
            .collect();

        assert_eq!(entries.len(), 4);
        assert!(!entries.iter().any(|(k, _)| *k == keys[2]));
    }
}
