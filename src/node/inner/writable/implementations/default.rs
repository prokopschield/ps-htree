use crate::node::inner::HtreeNodeWritable;

impl<T> Default for HtreeNodeWritable<T> {
    fn default() -> Self {
        Self::Empty
    }
}
