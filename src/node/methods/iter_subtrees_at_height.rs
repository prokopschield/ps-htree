use std::collections::VecDeque;

use ps_hkey::Store;

use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Iterates over all subtrees at exactly the specified height.
    ///
    /// Descends through the tree and yields every node whose height equals
    /// `height`, in left-to-right (sorted key) order. Implements
    /// [`DoubleEndedIterator`] for reverse (right-to-left) traversal.
    ///
    /// # Height matching
    ///
    /// This method uses **exact height matching**:
    /// - Nodes with `height == target` are yielded
    /// - Nodes with `height > target` are descended into
    /// - Nodes with `height < target` are skipped
    ///
    /// This means if a subtree is shorter than the requested height, it will
    /// not be yielded. For example, calling `iter_subtrees_at_height(2, &store)`
    /// on a leaf node (height 0) yields nothing.
    ///
    /// # Retry behavior
    ///
    /// On error, the iterator enters a retry state for the failed node: calling
    /// `next()` or `next_back()` again will attempt to refetch that node's children.
    ///
    /// # Errors
    ///
    /// See [`HtreeNodeIterSubtreesAtHeightError`] for all error variants.
    pub fn iter_subtrees_at_height<'s, S: Store>(
        &self,
        height: u8,
        store: &'s S,
    ) -> HtreeNodeIterSubtreesAtHeight<'s, T, S> {
        HtreeNodeIterSubtreesAtHeight {
            queue: VecDeque::from([self.clone()]),
            target_height: height,
            store,
        }
    }
}

/// Double-ended depth-first iterator over subtrees at exactly a specific height.
///
/// Maintains a deque of nodes to visit. `next()` processes from the front
/// (left-to-right), `next_back()` processes from the back (right-to-left).
/// The two directions maintain separate frontiers that meet in the middle.
pub struct HtreeNodeIterSubtreesAtHeight<'s, T, S: Store> {
    queue: VecDeque<HtreeNode<T>>,
    target_height: u8,
    store: &'s S,
}

impl<S: Store, T> Iterator for HtreeNodeIterSubtreesAtHeight<'_, T, S> {
    type Item = Result<HtreeNode<T>, HtreeNodeIterSubtreesAtHeightError<S>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let node = self.queue.pop_front()?;

            if node.is_empty() {
                continue;
            }

            let node_height = node.height();

            if node_height < self.target_height {
                continue;
            }

            if node_height == self.target_height {
                return Some(Ok(node));
            }

            // Descend: push children to front in reverse order (rightmost first)
            // so leftmost ends up at front
            let mut fetch_err = None;

            match node.fetch_children(self.store) {
                Ok(children) => {
                    for child in children.into_iter().rev() {
                        self.queue.push_front(child);
                    }
                }
                Err(err) => {
                    fetch_err = Some(err);
                }
            }

            if let Some(err) = fetch_err {
                // node not processed, return it into the queue
                self.queue.push_front(node);

                return Some(Err(err.into()));
            }
        }
    }
}

impl<S: Store, T> DoubleEndedIterator for HtreeNodeIterSubtreesAtHeight<'_, T, S> {
    fn next_back(&mut self) -> Option<Self::Item> {
        loop {
            let node = self.queue.pop_back()?;

            if node.is_empty() {
                continue;
            }

            let node_height = node.height();

            if node_height < self.target_height {
                continue;
            }

            if node_height == self.target_height {
                return Some(Ok(node));
            }

            // Descend: push children to back in order (leftmost first)
            // so rightmost ends up at back
            let mut fetch_err = None;

            match node.fetch_children(self.store) {
                Ok(children) => {
                    for child in children {
                        self.queue.push_back(child);
                    }
                }
                Err(err) => {
                    fetch_err = Some(err);
                }
            }

            if let Some(err) = fetch_err {
                // node not processed, return it into the queue
                self.queue.push_back(node);

                return Some(Err(err.into()));
            }
        }
    }
}

impl<S: Store, T> std::iter::FusedIterator for HtreeNodeIterSubtreesAtHeight<'_, T, S> {}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeIterSubtreesAtHeightError<S: Store> {
    #[error("HtreeNode's state is internally corrupted.")]
    CorruptedState,
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error(transparent)]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>>
    for HtreeNodeIterSubtreesAtHeightError<S>
{
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedState,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
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
    fn empty_tree_yields_no_subtrees() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<()> = HtreeNode::default();

        let subtrees: Vec<_> = tree.iter_subtrees_at_height(0, &store).collect();
        assert!(subtrees.is_empty());
    }

    #[test]
    fn single_leaf_at_height_zero() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");

        let subtrees: Vec<_> = tree
            .iter_subtrees_at_height(0, &store)
            .map(|r| r.expect("iter should not fail"))
            .collect();
        assert_eq!(subtrees.len(), 1);
        assert_eq!(subtrees[0].key, key);
    }

    #[test]
    fn single_leaf_at_higher_height_yields_nothing() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store)
            .expect("from_kvp should create a valid leaf node");

        let subtrees: Vec<_> = tree.iter_subtrees_at_height(1, &store).collect();
        assert!(subtrees.is_empty());
    }

    #[test]
    fn multi_leaf_tree_at_height_zero_yields_all_leaves() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4();
        let key2 = UUID::gen_v4();
        let key3 = UUID::gen_v4();

        let leaf1 =
            HtreeNode::<u64>::from_kvp(&key1, &1, &store).expect("from_kvp should create leaf1");
        let leaf2 =
            HtreeNode::<u64>::from_kvp(&key2, &2, &store).expect("from_kvp should create leaf2");
        let leaf3 =
            HtreeNode::<u64>::from_kvp(&key3, &3, &store).expect("from_kvp should create leaf3");

        let tree = HtreeNode::from_many_children([leaf1, leaf2, leaf3], &store)
            .expect("from_many_children should build tree")
            .into_iter()
            .next()
            .expect("should return at least one root");

        let subtrees: Vec<_> = tree
            .iter_subtrees_at_height(0, &store)
            .map(|r| r.expect("iter should not fail"))
            .collect();
        assert_eq!(subtrees.len(), 3);

        let collected_keys: Vec<_> = subtrees.iter().map(|s| s.key).collect();
        assert!(collected_keys.contains(&key1));
        assert!(collected_keys.contains(&key2));
        assert!(collected_keys.contains(&key3));
    }

    #[test]
    fn multi_leaf_tree_at_root_height_yields_root() {
        let store = InMemoryStore::default();

        let leaves: Vec<_> = (0..3)
            .map(|i| {
                let key = UUID::gen_v4();
                HtreeNode::<u64>::from_kvp(&key, &i, &store)
                    .expect("from_kvp should create leaf node")
            })
            .collect();

        let tree = HtreeNode::from_many_children(leaves, &store)
            .expect("from_many_children should build tree")
            .into_iter()
            .next()
            .expect("should return at least one root");

        let root_height = tree.height();

        let subtrees: Vec<_> = tree
            .iter_subtrees_at_height(root_height, &store)
            .map(|r| r.expect("iter should not fail"))
            .collect();
        assert_eq!(subtrees.len(), 1);
        assert_eq!(subtrees[0].key, tree.key);
    }

    #[test]
    fn subtrees_are_yielded_in_sorted_order() {
        let store = InMemoryStore::default();

        let mut original_keys: Vec<UUID> = (0..10).map(|_| UUID::gen_v4()).collect();

        let leaves: Vec<_> = original_keys
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
            .expect("should return at least one root");

        let keys: Vec<_> = tree
            .iter_subtrees_at_height(0, &store)
            .map(|r| r.expect("iter should not fail").key)
            .collect();

        let mut sorted_keys = keys.clone();
        sorted_keys.sort();
        assert_eq!(keys, sorted_keys);

        original_keys.sort();
        assert_eq!(keys, original_keys);
    }

    #[test]
    fn rev_yields_subtrees_in_reverse_sorted_order() {
        let store = InMemoryStore::default();

        let mut original_keys: Vec<UUID> = (0..10).map(|_| UUID::gen_v4()).collect();

        let leaves: Vec<_> = original_keys
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
            .expect("should return at least one root");

        let keys: Vec<_> = tree
            .iter_subtrees_at_height(0, &store)
            .rev()
            .map(|r| r.expect("iter should not fail").key)
            .collect();

        let mut sorted_keys_desc = keys.clone();
        sorted_keys_desc.sort();
        sorted_keys_desc.reverse();
        assert_eq!(keys, sorted_keys_desc);

        original_keys.sort();
        original_keys.reverse();
        assert_eq!(keys, original_keys);
    }

    #[test]
    fn interleaved_next_and_next_back() {
        let store = InMemoryStore::default();

        let mut original_keys: Vec<UUID> = (0..4).map(|_| UUID::gen_v4()).collect();
        original_keys.sort();

        let leaves: Vec<_> = original_keys
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
            .expect("should return at least one root");

        let mut iter = tree.iter_subtrees_at_height(0, &store);

        // Get first from front
        let first = iter.next().expect("should have first").expect("no error");
        assert_eq!(first.key, original_keys[0]);

        // Get last from back
        let last = iter
            .next_back()
            .expect("should have last")
            .expect("no error");
        assert_eq!(last.key, original_keys[3]);

        // Get second from front
        let second = iter.next().expect("should have second").expect("no error");
        assert_eq!(second.key, original_keys[1]);

        // Get third from back
        let third = iter
            .next_back()
            .expect("should have third")
            .expect("no error");
        assert_eq!(third.key, original_keys[2]);

        // Should be exhausted
        assert!(iter.next().is_none());
        assert!(iter.next_back().is_none());
    }
}
