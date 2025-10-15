mod implementations;

use ps_hkey::Hkey;

use crate::HtreeNode;

#[derive(Clone, Debug, Eq, PartialOrd, Ord)]
pub enum HtreeNodeWritable<T> {
    Empty,
    Internal { children: Vec<HtreeNode<T>> },
    Leaf { kvp: (Hkey, Hkey), value: Option<T> },
    Wrapped { hkey: Hkey },
}
