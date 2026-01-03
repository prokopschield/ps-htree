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

        let mut iter = children.into_iter();
        let mut nodes = Vec::with_capacity(num_groups);

        for _ in 0..num_groups {
            nodes.push(Self::from_children(iter.by_ref().take(group_size), store)?);
        }

        Ok(nodes)
    }
}
