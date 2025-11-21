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
