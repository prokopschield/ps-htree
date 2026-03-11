use std::ops::Deref;

use parking_lot::{MappedRwLockReadGuard, RwLockReadGuard};
use ps_hkey::Store;

use crate::{HtreeNode, HtreeNodeFetchChildrenError, node::inner::HtreeNodeWritable};

impl<T> HtreeNode<T> {
    /// Resolves and returns a borrowed view of this [`HtreeNode`]'s direct children.
    ///
    /// For internal nodes, resolves and returns a slice of children.
    /// For leaf or empty nodes, returns an empty slice.
    ///
    /// This avoids allocating and cloning child handles.
    ///
    /// # Arguments
    /// * `store` - persistence backend
    ///
    /// # Errors
    /// - [`HtreeNodeFetchChildrenError::CorruptedState`] is returned if this node's internal state is invalid.
    /// - [`HtreeNodeFetchChildrenError::Store`] is returned if persisted data cannot be accessed.
    /// - [`HtreeNodeFetchChildrenError::UnpackChildren`] is returned if child deserialization fails.
    pub fn fetch_children_guard<S: Store>(
        &self,
        store: &S,
    ) -> Result<HtreeChildrenGuard<'_, T>, HtreeNodeFetchChildrenError<S>> {
        if !self.is_leaf() {
            self.resolve(store)?;
        }

        RwLockReadGuard::try_map(self.read(), |state| match state {
            HtreeNodeWritable::Internal { children } => Some(children.as_slice()),
            HtreeNodeWritable::Empty | HtreeNodeWritable::Leaf => Some(&[]),
            HtreeNodeWritable::Wrapped => None,
        })
        .map(HtreeChildrenGuard)
        .map_err(|_| HtreeNodeFetchChildrenError::CorruptedState)
    }
}

/// A guard providing borrowed access to an [`HtreeNode`]'s children.
///
/// This guard holds a read lock on the node's internal state and dereferences
/// to a slice of child nodes. The lock is released when the guard is dropped.
pub struct HtreeChildrenGuard<'a, T>(MappedRwLockReadGuard<'a, [HtreeNode<T>]>);

impl<T> Deref for HtreeChildrenGuard<'_, T> {
    type Target = [HtreeNode<T>];

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::significant_drop_tightening)]
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn fetch_children_guard_returns_empty_slice_for_leaf() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let leaf = HtreeNode::<u64>::from_kvp(&key, &7, &store).expect("from_kvp should succeed");

        let children = leaf
            .fetch_children_guard(&store)
            .expect("fetch_children_guard should succeed");

        assert!(children.is_empty());
    }

    #[test]
    fn fetch_children_guard_returns_internal_children_slice() {
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
            .fetch_children_guard(&store)
            .expect("fetch_children_guard should succeed");
        let child_keys: Vec<_> = children.iter().map(|child| child.key).collect();

        assert_eq!(children.len(), 4);
        assert_eq!(child_keys, keys);
    }

    #[test]
    fn fetch_children_guard_matches_fetch_children_output() {
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

        let borrowed = parent
            .fetch_children_guard(&store)
            .expect("fetch_children_guard should succeed");
        let owned = parent
            .fetch_children(&store)
            .expect("fetch_children should succeed");

        assert_eq!(borrowed.len(), owned.len());
        assert!(
            borrowed
                .iter()
                .zip(owned.iter())
                .all(|(left, right)| left.key == right.key)
        );
    }
}
