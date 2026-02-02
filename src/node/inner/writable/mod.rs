use crate::HtreeNode;

#[derive(Clone, Debug, Default, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub enum HtreeNodeWritable<T> {
    #[default]
    Empty,
    Internal {
        children: Vec<HtreeNode<T>>,
    },
    Leaf,
    Wrapped,
}
