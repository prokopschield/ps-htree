use ps_hkey::Store;

use crate::{HtreeNode, HtreeNodeFromChildrenError, MAX_CHILDREN};

impl<T> HtreeNode<T> {
    /// Constructs parent nodes from an iterator of child nodes.
    ///
    /// Sorts children, increments height, groups by [`MAX_CHILDREN`],
    /// and hands off to [`Self::from_children`].
    ///
    /// # Errors
    ///
    /// See [`Self::from_children`].
    pub fn from_many_children<I, S>(
        children: I,
        store: &S,
    ) -> Result<Vec<Self>, HtreeNodeFromChildrenError<S>>
    where
        I: IntoIterator<Item = Self>,
        S: Store,
    {
        let mut children: Vec<Self> = children.into_iter().collect();

        if children.is_empty() {
            return Ok(Vec::new());
        }

        if children.len() <= MAX_CHILDREN {
            return Ok(vec![Self::from_children(children, store)?]);
        }

        children.sort();

        let num_groups = children.len().div_ceil(MAX_CHILDREN);
        let group_size = children.len().div_ceil(num_groups);

        let mut groups = Vec::with_capacity(num_groups);

        // Optimization: Iterate down to 1.
        // We handle the 0th index (the first group) after the loop
        // to reuse the original 'children' allocation.
        for i in (1..num_groups).rev() {
            // Efficient: this moves the tail elements into a new Vec,
            // leaving the head elements in place.
            let tail = children.split_off(i * group_size);

            groups.push(Self::from_children(tail, store)?);
        }

        // The remaining 'children' vec contains the first group [0..group_size].
        groups.push(Self::from_children(children, store)?);

        // We processed Last -> First, so reverse to get First -> Last.
        groups.reverse();

        Ok(groups)
    }
}
