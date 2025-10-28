use std::ops::Deref;

use ps_rwt::RWT;

use crate::{
    HtreeNode,
    node::inner::{HtreeNodeReadonly, HtreeNodeWritable},
};

impl<T> Deref for HtreeNode<T> {
    type Target = RWT<HtreeNodeReadonly, HtreeNodeWritable<T>>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
