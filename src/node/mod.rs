mod implementations;
mod inner;

use ps_rwt::RWT;

#[derive(Debug)]
pub struct HtreeNode<T> {
    inner: RWT<inner::HtreeNodeReadonly, inner::HtreeNodeWritable<T>>,
}
