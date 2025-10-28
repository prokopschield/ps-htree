use crate::HtreeNode;

impl<T> PartialEq for HtreeNode<T> {
    fn eq(&self, other: &Self) -> bool {
        self.hkey == other.hkey
    }
}
