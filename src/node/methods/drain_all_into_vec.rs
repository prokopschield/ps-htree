use ps_rwt::RWT;

use crate::{HtreeNode, node::inner::HtreeNodeWritable};

impl<T> HtreeNode<T> {
    /// Moves all descendant nodes from this tree into `target`.
    ///
    /// After this call the node becomes empty.
    /// Each non-empty leaf or wrapped node is moved into `target`.
    pub fn drain_all_into_vec(&self, target: &mut Vec<Self>) {
        let mut guard = self.write();
        let w = &mut *guard;

        match w {
            HtreeNodeWritable::Empty => {}
            HtreeNodeWritable::Internal { children } => {
                let children = std::mem::take(children);

                *w = HtreeNodeWritable::Empty;

                drop(guard); // prevent deadlock on recursion

                for child in children {
                    child.drain_all_into_vec(target);
                }
            }
            other => {
                let readonly = self.readonly();
                let writable = std::mem::replace(other, HtreeNodeWritable::Empty);

                target.push(Self {
                    inner: RWT::new(readonly, writable),
                });
            }
        }
    }
}
