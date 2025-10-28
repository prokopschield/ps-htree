use crate::HtreeNode;

impl<T> Clone for HtreeNode<T> {
    /// Creates a shallow clone of this node.
    ///
    /// This returns a new handle referencing the same
    /// underlying distributed node state, not a deep copy
    /// of its contents.
    ///
    /// To duplicate the tree’s contents, use
    /// [`HtreeNode::deep_clone`].
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
