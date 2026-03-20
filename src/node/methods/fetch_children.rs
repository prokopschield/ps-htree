use ps_hkey::Store;

use crate::{HtreeNode, node::inner::HtreeNodeWritable};

impl<T> HtreeNode<T> {
    /// Resolves and returns the direct children of this [`HtreeNode`].
    ///
    /// For internal nodes, resolves and returns children.
    /// For leaf nodes, returns an empty vector (leaves have no children).
    ///
    /// # Arguments
    /// * `store` - persistence backend
    ///
    /// # Errors
    /// - [`HtreeNodeFetchChildrenError::CorruptedState`] is returned if this node's internal state is invalid.
    /// - [`HtreeNodeFetchChildrenError::Store`] is returned if persisted data cannot be accessed.
    /// - [`HtreeNodeFetchChildrenError::UnpackChildren`] is returned if child deserialization fails.
    pub fn fetch_children<S: Store>(
        &self,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeFetchChildrenError<S>> {
        if self.is_leaf() {
            return Ok(vec![]);
        }

        self.resolve(store)?;

        match &*self.read() {
            HtreeNodeWritable::Empty => Ok(vec![]),
            HtreeNodeWritable::Internal { children } => Ok(children.clone()),
            HtreeNodeWritable::Leaf | HtreeNodeWritable::Wrapped => {
                // Leaf -> self.is_leaf() should have returned true
                // Wrapped -> self.resolve() should have resolved to Internal
                Err(HtreeNodeFetchChildrenError::CorruptedState)
            }
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeFetchChildrenError<S: Store> {
    #[error("HtreeNode's state is internally corrupted.")]
    CorruptedState,
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error(transparent)]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeResolveError<S>> for HtreeNodeFetchChildrenError<S> {
    fn from(value: crate::HtreeNodeResolveError<S>) -> Self {
        match value {
            crate::HtreeNodeResolveError::Store(err) => Self::Store(err),
            crate::HtreeNodeResolveError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::significant_drop_tightening)]
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn fetch_children_returns_empty_slice_for_leaf() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let leaf = HtreeNode::<u64>::from_kvp(&key, &7, &store).expect("from_kvp should succeed");

        let children = leaf
            .fetch_children(&store)
            .expect("fetch_children should succeed");

        assert!(children.is_empty());
    }

    #[test]
    fn fetch_children_returns_internal_children_slice() {
        let store = InMemoryStore::default();

        let mut keys: Vec<_> = (0..4).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                HtreeNode::<u64>::from_kvp(key, &(idx as u64), &store)
                    .expect("from_kvp should succeed")
            })
            .collect();
        let parent =
            HtreeNode::from_children(leaves, &store).expect("from_children should succeed");

        let children = parent
            .fetch_children(&store)
            .expect("fetch_children should succeed");
        let child_keys: Vec<_> = children.iter().map(|child| child.key).collect();

        assert_eq!(children.len(), 4);
        assert_eq!(child_keys, keys);
    }

    #[test]
    fn fetch_children_returns_empty_vec_for_default_node() {
        let store = InMemoryStore::default();
        let node = HtreeNode::<u64>::default();

        let children = node
            .fetch_children(&store)
            .expect("fetch_children should succeed");

        assert!(children.is_empty());
    }

    #[test]
    fn fetch_children_single_child() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let leaf = HtreeNode::<u64>::from_kvp(&key, &1, &store).expect("from_kvp should succeed");
        let parent =
            HtreeNode::from_children(vec![leaf], &store).expect("from_children should succeed");

        let children = parent
            .fetch_children(&store)
            .expect("fetch_children should succeed");

        assert_eq!(children.len(), 1);
        assert_eq!(children[0].key, key);
    }

    #[test]
    fn fetch_children_is_idempotent() {
        let store = InMemoryStore::default();

        let mut keys: Vec<_> = (0..3).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                HtreeNode::<u64>::from_kvp(key, &(idx as u64), &store)
                    .expect("from_kvp should succeed")
            })
            .collect();
        let parent =
            HtreeNode::from_children(leaves, &store).expect("from_children should succeed");

        let first: Vec<_> = parent
            .fetch_children(&store)
            .expect("first call should succeed")
            .iter()
            .map(|c| c.key)
            .collect();
        let second: Vec<_> = parent
            .fetch_children(&store)
            .expect("second call should succeed")
            .iter()
            .map(|c| c.key)
            .collect();

        assert_eq!(first, second);
    }

    #[test]
    fn fetch_children_returns_owned_data() {
        let store = InMemoryStore::default();

        let mut keys: Vec<_> = (0..3).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                HtreeNode::<u64>::from_kvp(key, &(idx as u64), &store)
                    .expect("from_kvp should succeed")
            })
            .collect();
        let parent =
            HtreeNode::from_children(leaves, &store).expect("from_children should succeed");

        let mut children = parent
            .fetch_children(&store)
            .expect("fetch_children should succeed");

        assert_eq!(children.len(), 3);

        children.pop();

        assert_eq!(children.len(), 2);

        let again = parent
            .fetch_children(&store)
            .expect("fetch_children should succeed");

        assert_eq!(again.len(), 3);
    }

    #[test]
    fn fetch_children_children_have_leaf_height() {
        let store = InMemoryStore::default();

        let mut keys: Vec<_> = (0..4).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                HtreeNode::<u64>::from_kvp(key, &(idx as u64), &store)
                    .expect("from_kvp should succeed")
            })
            .collect();
        let parent =
            HtreeNode::from_children(leaves, &store).expect("from_children should succeed");

        assert_eq!(parent.height(), 1);

        let children = parent
            .fetch_children(&store)
            .expect("fetch_children should succeed");

        for child in &children {
            assert_eq!(child.height(), 0);
            assert!(child.is_leaf());
        }
    }

    #[test]
    fn fetch_children_multilevel_tree() {
        let store = InMemoryStore::default();

        let mut keys: Vec<_> = (0..8).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                HtreeNode::<u64>::from_kvp(key, &(idx as u64), &store)
                    .expect("from_kvp should succeed")
            })
            .collect();

        let mid: Vec<_> = leaves
            .chunks(4)
            .map(|chunk| {
                HtreeNode::from_children(chunk.to_vec(), &store)
                    .expect("from_children should succeed")
            })
            .collect();
        let root = HtreeNode::from_children(mid, &store).expect("from_children should succeed");

        assert_eq!(root.height(), 2);

        let children = root
            .fetch_children(&store)
            .expect("fetch_children should succeed");

        assert_eq!(children.len(), 2);
        for child in &children {
            assert_eq!(child.height(), 1);
        }
    }

    #[test]
    fn fetch_children_matches_guard() {
        let store = InMemoryStore::default();

        let mut keys: Vec<_> = (0..4).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                HtreeNode::<u64>::from_kvp(key, &(idx as u64), &store)
                    .expect("from_kvp should succeed")
            })
            .collect();
        let parent =
            HtreeNode::from_children(leaves, &store).expect("from_children should succeed");

        let owned = parent
            .fetch_children(&store)
            .expect("fetch_children should succeed");
        let guard = parent
            .fetch_children_guard(&store)
            .expect("fetch_children_guard should succeed");

        assert_eq!(owned.len(), guard.len());
        assert!(
            owned
                .iter()
                .zip(guard.iter())
                .all(|(a, b)| a.key == b.key && a.height() == b.height())
        );
    }
}
