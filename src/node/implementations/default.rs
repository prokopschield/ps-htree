use ps_rwt::RWT;

use crate::HtreeNode;

impl<T> Default for HtreeNode<T> {
    fn default() -> Self {
        Self {
            inner: RWT::default(),
        }
    }
}
