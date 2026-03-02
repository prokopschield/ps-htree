use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode, HtreeValue};

impl<T: HtreeValue> HtreeNode<T> {
    /// Finds and returns the value associated with the given key.
    ///
    /// This is a convenience method that combines [`find_one`](Self::find_one)
    /// with value unpacking. Returns `Ok(Some(value))` if the key exists,
    /// `Ok(None)` if the key is not found.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to look up. Must implement [`HtreeKey`].
    /// * `store` - The persistence layer providing key conversion and value resolution.
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeFindOneValueError::Key`] if key conversion fails.
    /// - [`HtreeNodeFindOneValueError::Store`] if store operations fail.
    /// - [`HtreeNodeFindOneValueError::Unpack`] if value deserialization fails.
    /// - [`HtreeNodeFindOneValueError::UnpackChildren`] if unpacking child nodes fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ps_htree::HtreeNode;
    /// use ps_hkey::InMemoryStore;
    /// use ps_uuid::UUID;
    ///
    /// let store = InMemoryStore::default();
    /// let key = UUID::gen_v4();
    /// let value = 42_u64;
    ///
    /// let tree = HtreeNode::from_kvp(&key, &value, &store).unwrap();
    ///
    /// // Found key returns Some(value)
    /// assert_eq!(tree.find_one_value(&key, &store).unwrap(), Some(42));
    ///
    /// // Missing key returns None
    /// let other = UUID::gen_v4();
    /// assert_eq!(tree.find_one_value(&other, &store).unwrap(), None);
    /// ```
    pub fn find_one_value<K: HtreeKey + ?Sized, S: Store>(
        &self,
        key: &K,
        store: &S,
    ) -> Result<Option<T>, HtreeNodeFindOneValueError<T, S>> {
        let Some(leaf) = self.find_one(key, store)? else {
            return Ok(None);
        };

        let bytes = leaf
            .hkey
            .resolve(store)
            .map_err(HtreeNodeFindOneValueError::Store)?;

        let value = T::unpack_from_bytes(bytes, store)?;

        Ok(Some(value))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeFindOneValueError<T: HtreeValue, S: Store> {
    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),

    #[error("Store error: {0}")]
    Store(S::Error),

    #[error("Unpack error: {0}")]
    Unpack(T::UnpackError),

    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<T: HtreeValue, S: Store> From<crate::HtreeNodeFindOneError<S>>
    for HtreeNodeFindOneValueError<T, S>
{
    fn from(value: crate::HtreeNodeFindOneError<S>) -> Self {
        match value {
            crate::HtreeNodeFindOneError::Key(err) => Self::Key(err),
            crate::HtreeNodeFindOneError::Store(err) => Self::Store(err),
            crate::HtreeNodeFindOneError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

impl<T: HtreeValue, S: Store> From<crate::HtreeValueUnpackError<T, S>>
    for HtreeNodeFindOneValueError<T, S>
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
    fn get_from_empty_tree_returns_none() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<u64> = HtreeNode::default();
        let key = UUID::gen_v4();

        assert_eq!(
            tree.find_one_value(&key, &store)
                .expect("get should not fail on empty tree"),
            None
        );
    }

    #[test]
    fn get_existing_key_returns_value() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let value = 42_u64;

        let tree =
            HtreeNode::from_kvp(&key, &value, &store).expect("from_kvp should create a leaf node");

        assert_eq!(
            tree.find_one_value(&key, &store)
                .expect("get should succeed"),
            Some(42)
        );
    }

    #[test]
    fn get_missing_key_returns_none() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let other = UUID::gen_v4();
        let value = 42_u64;

        let tree =
            HtreeNode::from_kvp(&key, &value, &store).expect("from_kvp should create a leaf node");

        assert_eq!(
            tree.find_one_value(&other, &store)
                .expect("get should succeed"),
            None
        );
    }

    #[test]
    fn get_from_multi_leaf_tree() {
        let store = InMemoryStore::default();

        let keys: Vec<UUID> = (0..10).map(|_| UUID::gen_v4()).collect();
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

        // All keys should be retrievable with correct values
        for (i, key) in keys.iter().enumerate() {
            assert_eq!(
                tree.find_one_value(key, &store)
                    .expect("get should succeed"),
                Some(i as u64)
            );
        }

        // Missing key returns None
        let missing = UUID::gen_v4();
        assert_eq!(
            tree.find_one_value(&missing, &store)
                .expect("get should succeed"),
            None
        );
    }

    #[test]
    fn get_after_update() {
        let store = InMemoryStore::default();

        let keys: Vec<UUID> = (0..3).map(|_| UUID::gen_v4()).collect();
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

        assert_eq!(
            tree.find_one_value(&keys[1], &store)
                .expect("get should succeed"),
            Some(1)
        );

        let updated = HtreeNode::from_many_children(
            tree.update_one(&keys[1], &99_u64, &store)
                .expect("update_one should succeed"),
            &store,
        )
        .expect("from_many_children should succeed")
        .into_iter()
        .next()
        .unwrap_or_default();

        assert_eq!(
            updated
                .find_one_value(&keys[1], &store)
                .expect("get should succeed"),
            Some(99)
        );
    }

    #[test]
    fn get_after_deletion_returns_none() {
        let store = InMemoryStore::default();

        let keys: Vec<UUID> = (0..3).map(|_| UUID::gen_v4()).collect();
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
            .delete_one(&keys[1], &store)
            .expect("delete_one should succeed");

        // Deleted key returns None
        assert_eq!(
            tree.find_one_value(&keys[1], &store)
                .expect("get should succeed"),
            None
        );

        // Other keys still exist
        assert_eq!(
            tree.find_one_value(&keys[0], &store)
                .expect("get should succeed"),
            Some(0)
        );
        assert_eq!(
            tree.find_one_value(&keys[2], &store)
                .expect("get should succeed"),
            Some(2)
        );
    }
}
