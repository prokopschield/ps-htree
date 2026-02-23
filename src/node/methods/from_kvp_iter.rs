use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode, HtreeValue};

impl<V: HtreeValue> HtreeNode<V> {
    /// Creates a tree from an iterator of key-value pairs.
    ///
    /// This is a convenience constructor that creates leaves from the provided
    /// key-value pairs and combines them into a single tree root. Returns a
    /// default empty tree if the iterator is empty.
    ///
    /// # Arguments
    ///
    /// * `items` - Iterator of key-value pair references
    /// * `store` - Persistence layer for storing nodes
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeFromKvpIterError::Store`] if persistence fails.
    /// - [`HtreeNodeFromKvpIterError::Key`] if key conversion fails.
    /// - [`HtreeNodeFromKvpIterError::Pack`] if value serialization fails.
    /// - [`HtreeNodeFromKvpIterError::FromChildren`] if node construction fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ps_htree::HtreeNode;
    /// use ps_hkey::InMemoryStore;
    /// use ps_uuid::UUID;
    ///
    /// let store = InMemoryStore::default();
    /// let pairs: Vec<(UUID, u64)> = (0..5)
    ///     .map(|i| (UUID::gen_v4(), i as u64))
    ///     .collect();
    ///
    /// let items: Vec<_> = pairs.iter().map(|(k, v)| (k, v)).collect();
    /// let tree = HtreeNode::from_kvp_iter(items, &store).unwrap();
    ///
    /// assert_eq!(tree.count_leaves(&store).unwrap(), 5);
    /// ```
    pub fn from_kvp_iter<'k, 'v, K, I, S>(
        items: I,
        store: &S,
    ) -> Result<Self, HtreeNodeFromKvpIterError<V, S>>
    where
        K: HtreeKey + 'k,
        V: 'v,
        I: IntoIterator<Item = (&'k K, &'v V)>,
        S: Store,
    {
        let leaves: Vec<Self> = items
            .into_iter()
            .map(|(key, value)| Self::from_kvp(key, value, store))
            .collect::<Result<Vec<Self>, crate::HtreeNodeFromKvpError<V, S>>>()?;

        if leaves.is_empty() {
            return Ok(Self::default());
        }

        let mut nodes = Self::from_many_children(leaves, store)?;

        while nodes.len() > 1 {
            nodes = Self::from_many_children(nodes, store)?;
        }

        Ok(nodes.into_iter().next().unwrap_or_default())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeFromKvpIterError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    #[error(transparent)]
    FromChildren(crate::HtreeNodeFromChildrenError<S>),

    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),

    #[error("Pack error: {0}")]
    Pack(T::PackError),

    #[error("Store error: {0}")]
    Store(S::Error),
}

impl<T, S> From<crate::HtreeNodeFromKvpError<T, S>> for HtreeNodeFromKvpIterError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(err: crate::HtreeNodeFromKvpError<T, S>) -> Self {
        match err {
            crate::HtreeNodeFromKvpError::Store(err) => Self::Store(err),
            crate::HtreeNodeFromKvpError::Key(err) => err.into(),
            crate::HtreeNodeFromKvpError::Pack(err) => Self::Pack(err),
        }
    }
}

impl<T, S> From<crate::HtreeNodeFromChildrenError<S>> for HtreeNodeFromKvpIterError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(value: crate::HtreeNodeFromChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFromChildrenError::Store(err) => Self::Store(err),
            err => Self::FromChildren(err),
        }
    }
}

#[allow(unreachable_patterns)]
#[allow(clippy::match_wildcard_for_single_variants)]
impl<T, S> From<crate::HtreeKeyError<S>> for HtreeNodeFromKvpIterError<T, S>
where
    T: HtreeValue,
    S: Store,
{
    fn from(value: crate::HtreeKeyError<S>) -> Self {
        match value {
            crate::HtreeKeyError::Store(err) => Self::Store(err),
            err => Self::Key(err),
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
    fn from_kvp_iter_empty_returns_default() {
        let store = InMemoryStore::default();
        let items: Vec<(&UUID, &u64)> = Vec::new();

        let tree = HtreeNode::from_kvp_iter(items, &store).expect("from_kvp_iter should succeed");

        assert!(tree.is_empty());
    }

    #[test]
    fn from_kvp_iter_single_item() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let value = 42_u64;

        let tree = HtreeNode::from_kvp_iter([(&key, &value)], &store)
            .expect("from_kvp_iter should succeed");

        assert_eq!(
            tree.count_leaves(&store)
                .expect("count_leaves should succeed"),
            1
        );
        assert_eq!(
            tree.find_one_value(&key, &store)
                .expect("find_one_value should succeed"),
            Some(42)
        );
    }

    #[test]
    fn from_kvp_iter_multiple_items() {
        let store = InMemoryStore::default();

        let pairs: Vec<(UUID, u64)> = (0_u64..10).map(|i| (UUID::gen_v4(), i)).collect();
        let items: Vec<_> = pairs.iter().map(|(k, v)| (k, v)).collect();

        let tree = HtreeNode::from_kvp_iter(items, &store).expect("from_kvp_iter should succeed");

        assert_eq!(
            tree.count_leaves(&store)
                .expect("count_leaves should succeed"),
            10
        );

        // Verify all keys are present
        for (key, value) in &pairs {
            assert_eq!(
                tree.find_one_value(key, &store)
                    .expect("find_one_value should succeed"),
                Some(*value)
            );
        }
    }

    #[test]
    fn from_kvp_iter_large_input() {
        let store = InMemoryStore::default();

        let pairs: Vec<(UUID, u64)> = (0_u64..100).map(|i| (UUID::gen_v4(), i)).collect();
        let items: Vec<_> = pairs.iter().map(|(k, v)| (k, v)).collect();

        let tree = HtreeNode::from_kvp_iter(items, &store).expect("from_kvp_iter should succeed");

        assert_eq!(
            tree.count_leaves(&store)
                .expect("count_leaves should succeed"),
            100
        );
    }

    #[test]
    fn from_kvp_iter_preserves_values() {
        let store = InMemoryStore::default();

        let pairs: Vec<(UUID, u64)> = (0..20).map(|i| (UUID::gen_v4(), i * 10)).collect();
        let items: Vec<_> = pairs.iter().map(|(k, v)| (k, v)).collect();

        let tree = HtreeNode::from_kvp_iter(items, &store).expect("from_kvp_iter should succeed");

        // Get all entries and verify values
        for result in tree.iter_entries(&store) {
            let (key, value) = result.expect("iter_entries should not fail");

            // Find the original value for this key
            let original = pairs
                .iter()
                .find(|(k, _)| *k == key)
                .expect("key should exist in original pairs");

            assert_eq!(value, original.1);
        }
    }

    #[test]
    fn from_kvp_iter_keys_are_sorted() {
        let store = InMemoryStore::default();

        let pairs: Vec<(UUID, u64)> = (0_u64..15).map(|i| (UUID::gen_v4(), i)).collect();
        let items: Vec<_> = pairs.iter().map(|(k, v)| (k, v)).collect();

        let tree = HtreeNode::from_kvp_iter(items, &store).expect("from_kvp_iter should succeed");

        let keys: Vec<_> = tree
            .iter_keys(&store)
            .map(|r| r.expect("iter_keys should not fail"))
            .collect();

        let mut sorted = keys.clone();
        sorted.sort();

        assert_eq!(keys, sorted);
    }
}
