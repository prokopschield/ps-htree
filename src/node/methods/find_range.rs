use ps_hkey::Store;

use crate::{HtreeKey, HtreeNode};

impl<T> HtreeNode<T> {
    /// Traverses the tree to find all leaf nodes whose keys fall within the inclusive range [from, to].
    ///
    /// Performs a depth-first search, filtering at each level via child range selection,
    /// and returns only leaf nodes with keys in [from, to].
    ///
    /// # Arguments
    ///
    /// * `from` - Inclusive range start.
    /// * `to` - Inclusive range end.
    /// * `store` - Persistence layer for key conversion and child node resolution.
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeFindRangeError::Key`] is returned if conversion of `from` or `to` to UUID fails.
    /// - [`HtreeNodeFindRangeError::Store`] is returned if store operations fail.
    /// - [`HtreeNodeFindRangeError::UnpackChildren`] is returned if unpacking children fails during traversal.
    pub fn find_range<KFrom, KTo, S>(
        &self,
        from: &KFrom,
        to: &KTo,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeFindRangeError<S>>
    where
        KFrom: HtreeKey + ?Sized,
        KTo: HtreeKey + ?Sized,
        S: Store,
    {
        let from = from.try_to_uuid(store)?;
        let to = to.try_to_uuid(store)?;

        let mut queue = self.select_child_range(&from, &to, store)?;
        let mut results = Vec::new();

        // Reverse to implement DFS: pop from end processes initial children depth-first
        queue.reverse();

        while let Some(node) = queue.pop() {
            let mut subqueue = Vec::new();

            for child in node.select_child_range(&from, &to, store)? {
                if child.is_leaf() {
                    results.push(child);
                } else {
                    subqueue.push(child);
                }
            }

            // Reverse subqueue before extending to maintain left-to-right DFS order
            queue.extend(subqueue.into_iter().rev());
        }

        Ok(results)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeFindRangeError<S: Store> {
    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),

    #[error("Store error: {0}")]
    Store(S::Error),

    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

#[allow(unreachable_patterns)]
#[allow(clippy::match_wildcard_for_single_variants)]
impl<S: Store> From<crate::HtreeKeyError<S>> for HtreeNodeFindRangeError<S> {
    fn from(value: crate::HtreeKeyError<S>) -> Self {
        match value {
            crate::HtreeKeyError::Store(err) => Self::Store(err),
            err => Self::Key(err),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeSelectChildRangeError<S>> for HtreeNodeFindRangeError<S> {
    fn from(value: crate::HtreeNodeSelectChildRangeError<S>) -> Self {
        match value {
            crate::HtreeNodeSelectChildRangeError::Key(err) => err.into(),
            crate::HtreeNodeSelectChildRangeError::Store(err) => Self::Store(err),
            crate::HtreeNodeSelectChildRangeError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}
