mod implementations;
mod inner;
mod methods;

pub use methods::*;
use ps_rwt::RWT;

#[derive(Debug)]
pub struct HtreeNode<T> {
    inner: RWT<inner::HtreeNodeReadonly, inner::HtreeNodeWritable<T>>,
}
