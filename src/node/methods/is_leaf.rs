use crate::{HtreeNode, LEAF_HEIGHT};

impl<T> HtreeNode<T> {
    #[must_use]
    pub fn is_leaf(&self) -> bool {
        self.height == LEAF_HEIGHT
    }
}
