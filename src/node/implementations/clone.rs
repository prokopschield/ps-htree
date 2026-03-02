use crate::HtreeNode;

impl<T> Clone for HtreeNode<T> {
    /// Creates a shallow clone of this node.
    ///
    /// This returns a new handle referencing the same
    /// underlying distributed node state, not a deep copy
    /// of its contents.
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}
