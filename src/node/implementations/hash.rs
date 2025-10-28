use std::hash::Hash;

use crate::HtreeNode;

impl<T> Hash for HtreeNode<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.hkey.hash(state);
    }
}
