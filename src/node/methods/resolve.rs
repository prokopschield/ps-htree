use ps_hkey::Store;

use crate::{HtreeNode, HtreeNodeUnpackChildrenError, node::inner::HtreeNodeWritable};

impl<T> HtreeNode<T> {
    /// Resolves this [`HtreeNode`] by fetching its children from `store`.
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeResolveError::Store`] is returned if `self.hkey.resolve` fails.
    /// - [`HtreeNodeResolveError::UnpackChildren`] is returned if this node is malformed.
    pub fn resolve<S: Store>(&self, store: &S) -> Result<(), HtreeNodeResolveError<S>> {
        use HtreeNodeWritable::{Empty, Internal, Leaf};

        let mut guard = self.write();

        if let Empty | Internal { .. } | Leaf = *guard {
            return Ok(());
        }

        let raw_data = self
            .hkey
            .resolve(store)
            .map_err(HtreeNodeResolveError::Store)?;

        let children = Self::unpack_children(&raw_data)?;

        *guard = HtreeNodeWritable::Internal { children };

        drop(guard);

        Ok(())
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeResolveError<S: Store> {
    #[error("Store error: {0}")]
    Store(S::Error),
    #[error(transparent)]
    UnpackChildren(#[from] HtreeNodeUnpackChildrenError),
}
