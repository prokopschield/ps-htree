use std::cmp::Ordering;

use crate::HtreeNode;

impl<T> Ord for HtreeNode<T> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self.key.cmp(&other.key) {
            Ordering::Equal => self.hkey.cmp(&other.hkey),
            Ordering::Greater => Ordering::Greater,
            Ordering::Less => Ordering::Less,
        }
    }
}
