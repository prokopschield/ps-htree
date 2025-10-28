mod implementations;

use crate::HtreeNode;

#[derive(Clone, Debug)]
pub enum HtreeNodeWritable<T> {
    Empty,
    Internal { children: Vec<HtreeNode<T>> },
    Leaf,
    Wrapped,
}
