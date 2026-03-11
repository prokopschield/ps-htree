use ps_hkey::Store;

use crate::{HtreeChildrenGuard, HtreeNode};

impl<T> HtreeNode<T> {
    /// Resolves and returns an iterator over cloned children of this [`HtreeNode`].
    ///
    /// For internal nodes, resolves and returns an iterator that clones each child on demand.
    /// For leaf or empty nodes, returns an empty iterator.
    ///
    /// # Arguments
    /// * `store` - persistence backend
    ///
    /// # Errors
    /// - [`HtreeNodeIterChildrenError::CorruptedState`] is returned if this node's internal state is invalid.
    /// - [`HtreeNodeIterChildrenError::Store`] is returned if persisted data cannot be accessed.
    /// - [`HtreeNodeIterChildrenError::UnpackChildren`] is returned if child deserialization fails.
    pub fn iter_children<S: Store>(
        &self,
        store: &S,
    ) -> Result<HtreeNodeIterChildren<'_, T>, HtreeNodeIterChildrenError<S>> {
        self.fetch_children_guard(store)
            .map(HtreeNodeIterChildren::new)
            .map_err(Into::into)
    }
}

/// An iterator over cloned children of an [`HtreeNode`].
///
/// This iterator holds a read lock on the node's internal state and clones
/// each child on demand. The lock is released when the iterator is dropped.
pub struct HtreeNodeIterChildren<'a, T> {
    guard: HtreeChildrenGuard<'a, T>,
    front: usize,
    back: usize,
}

impl<'a, T> HtreeNodeIterChildren<'a, T> {
    fn new(guard: HtreeChildrenGuard<'a, T>) -> Self {
        let len = guard.len();

        Self {
            guard,
            front: 0,
            back: len,
        }
    }
}

impl<T> Iterator for HtreeNodeIterChildren<'_, T> {
    type Item = HtreeNode<T>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.front >= self.back {
            return None;
        }

        let item = self.guard.get(self.front)?.clone();

        self.front += 1;

        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.back - self.front;

        (len, Some(len))
    }
}

impl<T> DoubleEndedIterator for HtreeNodeIterChildren<'_, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front >= self.back {
            return None;
        }

        self.back -= 1;

        Some(self.guard.get(self.back)?.clone())
    }
}

impl<T> ExactSizeIterator for HtreeNodeIterChildren<'_, T> {}

impl<T> std::iter::FusedIterator for HtreeNodeIterChildren<'_, T> {}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeIterChildrenError<S: Store> {
    #[error("HtreeNode's state is internally corrupted.")]
    CorruptedState,
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error(transparent)]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodeIterChildrenError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedState,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]
    use std::convert::Infallible;

    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::{HtreeNode, HtreeValue, HtreeValueUnpackError};

    #[derive(Debug)]
    struct NonCloneValue(u64);

    #[derive(thiserror::Error, Debug)]
    enum NonCloneValueUnpackError {
        #[error("Cannot unpack {len} bytes into NonCloneValue (size {size}).")]
        TooManyBytes { len: usize, size: usize },
    }

    impl HtreeValue for NonCloneValue {
        type PackError = Infallible;
        type UnpackError = NonCloneValueUnpackError;

        fn pack_into<F, R, S>(
            &self,
            closure: F,
            _: &S,
        ) -> Result<R, crate::HtreeValuePackError<Self, S>>
        where
            F: FnOnce(&[u8]) -> R,
            S: ps_hkey::Store,
        {
            let bytes = self.0.to_be_bytes();

            let Some(index) = bytes.iter().position(|&byte| byte != 0) else {
                return Ok(closure(&[]));
            };

            Ok(closure(&bytes[index..]))
        }

        fn unpack<S: ps_hkey::Store>(
            bytes: &[u8],
            _: &S,
        ) -> Result<Self, HtreeValueUnpackError<Self, S>> {
            let len = bytes.len();
            let size = std::mem::size_of::<u64>();
            let mut array = [0u8; std::mem::size_of::<u64>()];

            if len <= size {
                array[size - len..].copy_from_slice(bytes);
                Ok(Self(u64::from_be_bytes(array)))
            } else {
                Err(HtreeValueUnpackError::Unpack(
                    NonCloneValueUnpackError::TooManyBytes { len, size },
                ))
            }
        }
    }

    #[test]
    fn iter_children_returns_empty_for_leaf() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();
        let leaf = HtreeNode::<u64>::from_kvp(&key, &7, &store).expect("from_kvp should succeed");

        let children: Vec<_> = leaf
            .iter_children(&store)
            .expect("iter_children should succeed")
            .collect();

        assert!(children.is_empty());
    }

    #[test]
    fn iter_children_yields_cloned_children() {
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

        let children: Vec<_> = parent
            .iter_children(&store)
            .expect("iter_children should succeed")
            .collect();
        let child_keys: Vec<_> = children.iter().map(|child| child.key).collect();

        assert_eq!(children.len(), 4);
        assert_eq!(child_keys, keys);
    }

    #[test]
    fn iter_children_double_ended() {
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

        let children: Vec<_> = parent
            .iter_children(&store)
            .expect("iter_children should succeed")
            .rev()
            .collect();
        let child_keys: Vec<_> = children.iter().map(|child| child.key).collect();

        let mut expected_keys = keys.clone();
        expected_keys.reverse();
        assert_eq!(child_keys, expected_keys);
    }

    #[test]
    fn iter_children_works_with_non_clone_value() {
        let store = InMemoryStore::default();

        let mut keys: Vec<_> = (0..3).map(|_| UUID::gen_v4()).collect();
        keys.sort();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(idx, key)| {
                HtreeNode::from_kvp(key, &NonCloneValue(idx as u64), &store)
                    .expect("from_kvp should succeed")
            })
            .collect();
        let parent =
            HtreeNode::from_children(leaves, &store).expect("from_children should succeed");

        let children: Vec<_> = parent
            .iter_children(&store)
            .expect("iter_children should succeed")
            .collect();
        let child_keys: Vec<_> = children.iter().map(|child| child.key).collect();

        assert_eq!(children.len(), 3);
        assert_eq!(child_keys, keys);
    }
}
