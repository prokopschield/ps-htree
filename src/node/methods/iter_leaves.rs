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
