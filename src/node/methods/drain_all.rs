use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Removes all descendant nodes from this tree and returns them.
    ///
    /// After this call the node becomes empty.
    /// Each non-empty leaf or wrapped node returned as-is.
    #[must_use]
    pub fn drain_all(&self) -> Vec<Self> {
        let mut target = Vec::new();

        self.drain_all_into_vec(&mut target);

        target
    }
}
