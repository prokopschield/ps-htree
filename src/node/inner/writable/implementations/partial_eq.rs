use crate::node::inner::HtreeNodeWritable;

impl<T> PartialEq for HtreeNodeWritable<T> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Empty, Self::Empty) => true,

            (Self::Empty, Self::Internal { children })
            | (Self::Internal { children }, Self::Empty) => children.is_empty(),

            (Self::Internal { children: lhs }, Self::Internal { children: rhs }) => lhs == rhs,

            (Self::Leaf { kvp: lhs_kvp, .. }, Self::Leaf { kvp: rhs_kvp, .. }) => {
                lhs_kvp == rhs_kvp
            }

            (Self::Wrapped { hkey: lhs }, Self::Wrapped { hkey: rhs }) => lhs == rhs,

            _ => false,
        }
    }
}
