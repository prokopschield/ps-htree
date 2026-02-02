use ps_hkey::Store;

use crate::{HtreeNode, HtreeNodeFetchChildrenError};

impl<T> HtreeNode<T> {
    /// Returns the leaf with the smallest key in the tree.
    ///
    /// Descends the leftmost path from root to leaf with O(height) traversal,
    /// fetching O(1) nodes per level.
    ///
    /// Returns `None` only if the tree is empty (i.e., `is_empty()` is true).
    /// For a non-empty tree, always returns `Some(leaf)`.
    ///
    /// # Arguments
    /// * `store` - persistence backend
    ///
    /// # Errors
    /// Returns an error if children cannot be fetched during traversal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ps_hkey::InMemoryStore;
    /// use ps_uuid::UUID;
    /// use ps_htree::HtreeNode;
    ///
    /// let store = InMemoryStore::default();
    ///
    /// // Empty tree returns None
    /// let empty: HtreeNode<()> = HtreeNode::default();
    /// assert!(empty.first(&store).unwrap().is_none());
    ///
    /// // Single leaf returns itself
    /// let key = UUID::gen_v4().with_version(8);
    /// let leaf = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
    /// let first = leaf.first(&store).unwrap().unwrap();
    /// assert_eq!(first.key, key);
    /// ```
    pub fn first<S: Store>(
        &self,
        store: &S,
    ) -> Result<Option<Self>, HtreeNodeFetchChildrenError<S>> {
        if self.is_empty() {
            return Ok(None);
        }

        let mut current = self.clone();

        while !current.is_leaf() {
            let children = current.fetch_children(store)?;

            // Children are sorted; first child contains the minimum key
            current = match children.into_iter().next() {
                Some(child) => child,
                None => return Ok(None),
            };
        }

        Ok(Some(current))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use ps_hkey::InMemoryStore;
    use ps_uuid::UUID;

    use crate::HtreeNode;

    #[test]
    fn empty_tree_returns_none() {
        let store = InMemoryStore::default();
        let tree: HtreeNode<()> = HtreeNode::default();
        assert!(tree.first(&store).unwrap().is_none());
    }

    #[test]
    fn single_leaf_returns_itself() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let tree = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
        let first = tree.first(&store).unwrap().unwrap();

        assert_eq!(first.key, key);
        assert!(first.is_leaf());
    }

    #[test]
    fn two_leaves_returns_smaller_key() {
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

        let first = tree.first(&store).unwrap().unwrap();
        let expected_key = std::cmp::min(key1, key2);

        assert_eq!(first.key, expected_key);
        assert!(first.is_leaf());
    }

    #[test]
    fn many_leaves_returns_smallest_key() {
        let store = InMemoryStore::default();

        // Generate multiple keys
        let keys: Vec<_> = (0..10).map(|_| UUID::gen_v4().with_version(8)).collect();

        let leaves: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| HtreeNode::<u64>::from_kvp(k, &(i as u64), &store).unwrap())
            .collect();

        let tree = HtreeNode::from_many_children(leaves, &store)
            .unwrap()
            .into_iter()
            .next()
            .unwrap();

        let first = tree.first(&store).unwrap().unwrap();
        let expected_key = *keys.iter().min().unwrap();

        assert_eq!(first.key, expected_key);
        assert!(first.is_leaf());
    }

    #[test]
    fn first_on_leaf_returns_self() {
        let store = InMemoryStore::default();
        let key = UUID::gen_v4().with_version(8);

        let leaf = HtreeNode::<u64>::from_kvp(&key, &42, &store).unwrap();
        assert!(leaf.is_leaf());

        let first = leaf.first(&store).unwrap().unwrap();
        assert_eq!(first.key, leaf.key);
    }
}
