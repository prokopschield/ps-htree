use ps_hkey::Store;

use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Iterates over all leaf nodes in this tree using depth-first traversal.
    ///
    /// Yields leaves in left-to-right order. On error, the iterator enters
    /// a retry state for the failed node: calling `next()` again will attempt
    /// to refetch the failed node's children.
    ///
    /// # Arguments
    /// * `store` - Persistence backend
    ///
    /// # Errors
    /// See [`HtreeNodeIterLeavesError`] for all error variants.
    pub fn iter_leaves<'s, S: Store>(&self, store: &'s S) -> HtreeNodeIterLeaves<'s, T, S> {
        HtreeNodeIterLeaves {
            queue: vec![self.clone()],
            store,
        }
    }
}

/// Depth-first iterator over leaf nodes.
///
/// Maintains a stack-based queue of internal nodes to visit. On each call
/// to `next()`, pops a node; if internal, fetches children and reverses
/// them to maintain left-to-right yield order.
pub struct HtreeNodeIterLeaves<'s, T, S: Store> {
    queue: Vec<HtreeNode<T>>,
    store: &'s S,
}

impl<S: Store, T> Iterator for HtreeNodeIterLeaves<'_, T, S> {
    type Item = Result<HtreeNode<T>, HtreeNodeIterLeavesError<S>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let next = self.queue.pop()?;

            if next.is_empty() {
                continue;
            }

            if next.is_leaf() {
                return Some(Ok(next));
            }

            match next.fetch_children(self.store) {
                Ok(children) => self.queue.extend(children.into_iter().rev()),
                Err(err) => {
                    // node not processed -> keep iterator state consistent
                    self.queue.push(next);

                    return Some(Err(err.into()));
                }
            }
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeIterLeavesError<S: Store> {
    #[error("HtreeNode's state is internally corrupted.")]
    CorruptedState,
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error(transparent)]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

impl<S: Store> From<crate::HtreeNodeFetchChildrenError<S>> for HtreeNodeIterLeavesError<S> {
    fn from(value: crate::HtreeNodeFetchChildrenError<S>) -> Self {
        match value {
            crate::HtreeNodeFetchChildrenError::CorruptedState => Self::CorruptedState,
            crate::HtreeNodeFetchChildrenError::Store(err) => Self::Store(err),
            crate::HtreeNodeFetchChildrenError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn empty_tree_yields_no_leaves() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<()> = HtreeNode::default();

        let leaves: Vec<_> = tree.iter_leaves(&store).collect();
        assert!(leaves.is_empty());
    }

    #[test]
    fn single_leaf_yields_itself() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4();

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();

        let leaves: Vec<_> = tree.iter_leaves(&store).map(|r| r.unwrap()).collect();
        assert_eq!(leaves.len(), 1);
        assert_eq!(leaves[0].key, key);
    }

    #[test]
    fn multi_leaf_tree_yields_all_leaves() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4();
        let key2 = UUID::gen_v4();
        let key3 = UUID::gen_v4();

        let leaf1 = HtreeNode::<u64>::from_kvp(&key1, &1, &store).unwrap();
        let leaf2 = HtreeNode::<u64>::from_kvp(&key2, &2, &store).unwrap();
        let leaf3 = HtreeNode::<u64>::from_kvp(&key3, &3, &store).unwrap();

        let tree = HtreeNode::from_many_children([leaf1, leaf2, leaf3], &store)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        let leaves: Vec<_> = tree.iter_leaves(&store).map(|r| r.unwrap()).collect();
        assert_eq!(leaves.len(), 3);

        let keys: Vec<_> = leaves.iter().map(|l| l.key).collect();
        assert!(keys.contains(&key1));
        assert!(keys.contains(&key2));
        assert!(keys.contains(&key3));
    }

    #[test]
    fn leaves_are_yielded_in_sorted_order() {
        let store = InMemoryStore::default();

        let mut original_keys: Vec<UUID> = (0..10).map(|_| UUID::gen_v4()).collect();

        let leaves: Vec<_> = original_keys
            .iter()
            .enumerate()
            .map(|(i, k)| HtreeNode::<u64>::from_kvp(k, &(i as u64), &store).unwrap())
            .collect();

        let tree = HtreeNode::from_many_children(leaves, &store)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        let keys: Vec<_> = tree.iter_leaves(&store).map(|r| r.unwrap().key).collect();

        let mut sorted_keys = keys.clone();
        sorted_keys.sort();
        assert_eq!(keys, sorted_keys);

        original_keys.sort();
        assert_eq!(keys, original_keys);
    }
}
