use crate::HtreeNode;

impl<T> PartialOrd for HtreeNode<T> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
