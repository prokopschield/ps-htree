use crate::HtreeNode;

impl<T> HtreeNode<T> {
    #[must_use]
    pub fn is_leaf(&self) -> bool {
        self.height == 0
    }
}
