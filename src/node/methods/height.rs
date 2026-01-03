use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Returns the height of this tree node.
    ///
    /// The height is the number of edges from this node to its descendant leaves.
    /// A leaf node has height `0`. The subtree rooted at this node can store up to
    /// `ps_htree::MAX_CHILDREN.pow(height as u32)` total elements.
    ///
    /// # Examples
    ///
    /// ```
    /// use ps_htree::{MAX_CHILDREN, HtreeNode};
    /// let node = HtreeNode::<()>::default();
    /// assert_eq!(node.height(), 0);
    ///
    /// let capacity = MAX_CHILDREN.pow(node.height() as u32);
    /// assert_eq!(capacity, 1);
    /// ```
    #[inline]
    #[must_use]
    pub fn height(&self) -> u8 {
        self.height
    }
}
