use std::hash::Hash;

use crate::node::inner::HtreeNodeWritable;

impl<T> Hash for HtreeNodeWritable<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Self::Empty => {}
            Self::Internal { children } => children.hash(state),
            Self::Leaf { kvp, .. } => kvp.hash(state),
            Self::Wrapped { hkey } => hkey.hash(state),
        }
    }
}
