use ps_hkey::Store;
use ps_util::Array;

use crate::{HtreeKey, HtreeNode, node::inner::HtreeNodeWritable};

impl<T> HtreeNode<T> {
    /// Selects all child nodes whose key ranges may contain values within the inclusive range [from, to].
    ///
    /// For internal nodes, returns all whose ranges might overlap [from, to].
    /// For leaf nodes, returns only those whose keys fall within [from, to].
    ///
    /// # Arguments
    ///
    /// * `from` - The inclusive start of the key range. Must implement [`HtreeKey`] for UUID conversion.
    /// * `to` - The inclusive end of the key range. Must implement [`HtreeKey`] for UUID conversion.
    /// * `store` - The persistence layer providing key conversion and child node resolution.
    ///
    /// # Errors
    ///
    /// - [`HtreeNodeSelectChildRangeError::Key`] is returned if conversion of `from` or `to` to a UUID fails.
    /// - [`HtreeNodeSelectChildRangeError::Store`] is returned if store operations fail during key conversion or child node retrieval.
    /// - [`HtreeNodeSelectChildRangeError::UnpackChildren`] is returned if unpacking child nodes fails, indicating corrupted or invalid persisted state.
    pub fn select_child_range<S: Store>(
        &self,
        from: &impl HtreeKey,
        to: &impl HtreeKey,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeSelectChildRangeError<S>> {
        let from = from.try_to_uuid(store)?;
        let to = to.try_to_uuid(store)?;

        // Leaf nodes: return self only if key is within [from, to] inclusive
        if self.is_leaf() {
            if self.key >= from && self.key <= to {
                return Ok(vec![self.clone()]);
            }
            return Ok(vec![]);
        }

        self.resolve(store)?;

        let guard = self.read();

        // Return empty if node is corrupted (expected internal but isn't)
        let HtreeNodeWritable::Internal { children } = &*guard else {
            return Ok(vec![]);
        };

        // Empty node/range yields no children
        if from > to || children.is_empty() {
            return Ok(vec![]);
        }

        // Find first child whose range may contain keys >= from_uuid.
        // partition_point returns first index where condition is false, so we subtract 1
        // to get the last child where key <= from_uuid, which contains the `from` boundary.
        let first_index = children
            .partition_point(|child| child.key <= from)
            .saturating_sub(1);

        // Find last child whose range may contain keys <= to_uuid.
        // Same logic: last child where key <= to_uuid.
        let last_index = children
            .partition_point(|child| child.key <= to)
            .saturating_sub(1);

        // Defensive early return, protects against corrupted child lists
        if last_index < first_index {
            return Ok(vec![]);
        }

        // Collect children in range [first_index, last_index]
        let result = children[first_index..=last_index].to_vec();

        drop(guard);

        // Check whether the query falls entirely before the first child's key
        if let Some(first_item) = result.first()
            && first_item.key > to
        {
            return Ok(vec![]);
        }

        // Filter leaves outside of range
        let result = result.filter(|item| !item.is_leaf() || (from <= item.key && item.key <= to));

        Ok(result)
    }
}

#[derive(thiserror::Error, Debug)]
pub enum HtreeNodeSelectChildRangeError<S: Store> {
    #[error("Key error: {0}")]
    Key(crate::HtreeKeyError<S>),

    #[error("Store error: {0}")]
    Store(S::Error),

    #[error("Error unpacking children: {0}")]
    UnpackChildren(#[from] crate::HtreeNodeUnpackChildrenError),
}

#[allow(unreachable_patterns)]
#[allow(clippy::match_wildcard_for_single_variants)]
impl<S: Store> From<crate::HtreeKeyError<S>> for HtreeNodeSelectChildRangeError<S> {
    fn from(value: crate::HtreeKeyError<S>) -> Self {
        match value {
            crate::HtreeKeyError::Store(err) => Self::Store(err),
            _ => Self::Key(value),
        }
    }
}

impl<S: Store> From<crate::HtreeNodeResolveError<S>> for HtreeNodeSelectChildRangeError<S> {
    fn from(value: crate::HtreeNodeResolveError<S>) -> Self {
        match value {
            crate::HtreeNodeResolveError::Store(err) => Self::Store(err),
            crate::HtreeNodeResolveError::UnpackChildren(err) => Self::UnpackChildren(err),
        }
    }
}
