use ps_hkey::Hkey;
use ps_uuid::UUID;

use crate::HtreeNode;

impl<T> HtreeNode<T> {
    /// Returns `true` if this tree is logically empty.
    ///
    /// A tree is empty when it is structurally equivalent to [`Default::default()`]:
    /// height is `0`, key is [`UUID::nil()`], and hkey is [`Hkey::Empty`].
    ///
    /// This is a pure structural check with O(1) complexity and requires no store access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ps_htree::HtreeNode;
    ///
    /// let empty: HtreeNode<()> = HtreeNode::default();
    /// assert!(empty.is_empty());
    /// ```
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.height == 0 && self.key == UUID::nil() && self.hkey == Hkey::Empty
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn default_is_empty() {
        let tree: HtreeNode<()> = HtreeNode::default();
        assert!(tree.is_empty());
    }

    #[test]
    fn single_leaf_is_not_empty() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
        assert!(!tree.is_empty());
    }

    #[test]
    fn multi_leaf_tree_is_not_empty() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4().with_version(8);
        let key2 = UUID::gen_v4().with_version(8);

        let leaf1 = HtreeNode::<u64>::from_kvp(&key1, &1, &store).unwrap();
        let leaf2 = HtreeNode::<u64>::from_kvp(&key2, &2, &store).unwrap();

        let tree = HtreeNode::from_many_children([leaf1, leaf2], &store)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        assert!(!tree.is_empty());
    }

    #[test]
    fn tree_after_deleting_all_leaves_is_empty() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
        assert!(!tree.is_empty());

        let tree = tree.delete_one(&key, &store).unwrap();
        assert!(tree.is_empty());
    }

    #[test]
    fn tree_after_partial_deletion_is_not_empty() {
        let store = InMemoryStore::default();
        let key1 = UUID::gen_v4().with_version(8);
        let key2 = UUID::gen_v4().with_version(8);

        let leaf1 = HtreeNode::<u64>::from_kvp(&key1, &1, &store).unwrap();
        let leaf2 = HtreeNode::<u64>::from_kvp(&key2, &2, &store).unwrap();

        let tree = HtreeNode::from_many_children([leaf1, leaf2], &store)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        let tree = tree.delete_one(&key1, &store).unwrap();
        assert!(!tree.is_empty());
    }
}
