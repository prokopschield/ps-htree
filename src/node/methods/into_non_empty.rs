use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Returns `Some(self)` if this node is non-empty, or `None` if it is empty.
    #[must_use]
    pub fn into_non_empty(self) -> Option<Self> {
        if self.is_empty() { None } else { Some(self) }
    }
}
