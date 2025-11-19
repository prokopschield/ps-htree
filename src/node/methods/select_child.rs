use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode, node::inner::HtreeNodeWritable};

impl<T> HtreeNode<T> {
    /// Selects a child from an internal node
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeSelectChildError::Key`] is passed from [`HtreeKey::try_to_uuid`]
    /// - [`HtreeNodeSelectChildError::Store`] is passed from store operations
    /// - [`HtreeNodeSelectChildError::UnpackChildren`] is passed from [`Self::resolve`]
    pub fn select_child<S: Store>(
        &self,
        key: &impl HtreeKey,
        store: &S,
    ) -> Result<Option<Self>, HtreeNodeSelectChildError<S>> {
        // resolve key to UUID
        let key = key.try_to_uuid(store)?;

        // return self on match
        if self.is_leaf() {
            if self.key == key {
                return Ok(Some(self.clone()));
            }
            return Ok(None);
        }

        // make sure node is not wrapped
        self.resolve(store)?;

        // nodes are immutable, no waiting here
        let guard = self.read();

        // if this node is empty, return None
        let HtreeNodeWritable::Internal { children } = &*guard else {
            return Ok(None);
        };

        let index = children
            .partition_point(|child| child.key <= key)
            .saturating_sub(1);

        let Some(child) = children.get(index).cloned() else {
            return Ok(None);
        };

        drop(guard);

        Ok(Some(child))
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeSelectChildError<S: Store> {
    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

#[allow(unreachable_patterns)]
#[allow(clippy::match_wildcard_for_single_variants)]
impl<S: Store> From<crate::HtreeKeyError<S>> for HtreeNodeSelectChildError<S> {
    fn from(value: crate::HtreeKeyError<S>) -> Self {
        match value {
            crate::HtreeKeyError::Store(err) => Self::Store(err),
            _ => Self::Key(value),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeResolveError<S>> for HtreeNodeSelectChildError<S> {
    fn from(value: crate::HtreeNodeResolveError<S>) -> Self {
        match value {
            crate::HtreeNodeResolveError::Store(err) => Self::Store(err),
            crate::HtreeNodeResolveError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}
