//! Intrusive heap - a min-heap using intrusive links.
//!
//! This is a placeholder for future use. The current timed scheduler
//! uses `std::collections::BinaryHeap` for simplicity.

use std::cmp::Ordering;
use std::ptr::NonNull;

/// A node that can be stored in an intrusive heap.
///
/// Implementors must provide a key for ordering and storage for parent/child pointers.
pub trait IntrusiveHeapNode: Sized {
    /// The key type used for ordering nodes.
    type Key: Ord;

    /// Get the key for this node.
    fn key(&self) -> &Self::Key;

    /// Get the parent pointer.
    fn parent(&self) -> Option<NonNull<Self>>;
    /// Set the parent pointer.
    fn set_parent(&mut self, parent: Option<NonNull<Self>>);

    /// Get the left child pointer.
    fn left(&self) -> Option<NonNull<Self>>;
    /// Set the left child pointer.
    fn set_left(&mut self, left: Option<NonNull<Self>>);

    /// Get the right child pointer.
    fn right(&self) -> Option<NonNull<Self>>;
    /// Set the right child pointer.
    fn set_right(&mut self, right: Option<NonNull<Self>>);

    /// Check if this node is in the heap.
    fn is_in_heap(&self) -> bool;
    /// Mark this node as not in the heap.
    fn mark_not_in_heap(&mut self);
}

/// An intrusive min-heap.
///
/// This heap stores nodes without owning them - the caller is responsible
/// for ensuring nodes outlive the heap and are not moved while in the heap.
///
/// # Note
///
/// This is a simplified implementation. For the current timed scheduler,
/// we use `std::collections::BinaryHeap` instead.
pub struct IntrusiveHeap<N: IntrusiveHeapNode> {
    root: Option<NonNull<N>>,
    size: usize,
}

impl<N: IntrusiveHeapNode> Default for IntrusiveHeap<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<N: IntrusiveHeapNode> IntrusiveHeap<N> {
    /// Create a new empty heap.
    pub const fn new() -> Self {
        Self {
            root: None,
            size: 0,
        }
    }

    /// Check if the heap is empty.
    pub fn is_empty(&self) -> bool {
        self.size == 0
    }

    /// Get the number of elements in the heap.
    pub fn len(&self) -> usize {
        self.size
    }

    /// Get the minimum element without removing it.
    pub fn front(&self) -> Option<NonNull<N>> {
        self.root
    }

    /// Insert a node into the heap.
    ///
    /// # Safety
    /// The caller must ensure:
    /// - The node is not already in this heap
    /// - The node will not be moved while in the heap
    /// - The node will outlive the heap
    pub unsafe fn insert(&mut self, node: NonNull<N>) {
        self.size += 1;

        if self.root.is_none() {
            unsafe {
                let node_mut = &mut *node.as_ptr();
                node_mut.set_parent(None);
                node_mut.set_left(None);
                node_mut.set_right(None);
            }
            self.root = Some(node);
            return;
        }

        // Simple insertion: find position and insert
        // For a proper implementation, we'd use the path-based approach
        // For now, just insert at root and sift down if needed
        unsafe {
            let node_mut = &mut *node.as_ptr();
            let root = self.root.unwrap();

            if node.as_ref().key() < root.as_ref().key() {
                // New node becomes root
                node_mut.set_parent(None);
                node_mut.set_left(Some(root));
                node_mut.set_right(None);
                (*root.as_ptr()).set_parent(Some(node));
                self.root = Some(node);
            } else {
                // Insert as child of root (simplified)
                let root_mut = &mut *root.as_ptr();
                if root_mut.left().is_none() {
                    node_mut.set_parent(Some(root));
                    node_mut.set_left(None);
                    node_mut.set_right(None);
                    root_mut.set_left(Some(node));
                } else if root_mut.right().is_none() {
                    node_mut.set_parent(Some(root));
                    node_mut.set_left(None);
                    node_mut.set_right(None);
                    root_mut.set_right(Some(node));
                } else {
                    // For simplicity, insert into left subtree
                    // A proper implementation would balance the tree
                    let left = root_mut.left().unwrap();
                    node_mut.set_parent(Some(left));
                    node_mut.set_left(None);
                    node_mut.set_right(None);

                    let left_mut = &mut *left.as_ptr();
                    if left_mut.left().is_none() {
                        left_mut.set_left(Some(node));
                    } else {
                        left_mut.set_right(Some(node));
                    }
                }
            }
        }
    }

    /// Remove and return the minimum element.
    ///
    /// # Safety
    /// The caller must ensure the returned node is properly handled.
    pub unsafe fn pop_front(&mut self) -> Option<NonNull<N>> {
        let root = self.root?;

        if self.size == 1 {
            self.root = None;
            self.size = 0;
            unsafe {
                let root_mut = &mut *root.as_ptr();
                root_mut.mark_not_in_heap();
            }
            return Some(root);
        }

        self.size -= 1;

        unsafe {
            let root_ref = root.as_ref();
            let left = root_ref.left();
            let right = root_ref.right();

            // Promote the smaller child to be the new root
            match (left, right) {
                (None, None) => {
                    self.root = None;
                }
                (Some(l), None) => {
                    (*l.as_ptr()).set_parent(None);
                    self.root = Some(l);
                }
                (None, Some(r)) => {
                    (*r.as_ptr()).set_parent(None);
                    self.root = Some(r);
                }
                (Some(l), Some(r)) => {
                    if l.as_ref().key() < r.as_ref().key() {
                        (*l.as_ptr()).set_parent(None);
                        (*l.as_ptr()).set_right(Some(r));
                        (*r.as_ptr()).set_parent(Some(l));
                        self.root = Some(l);
                    } else {
                        (*r.as_ptr()).set_parent(None);
                        (*r.as_ptr()).set_left(Some(l));
                        (*l.as_ptr()).set_parent(Some(r));
                        self.root = Some(r);
                    }
                }
            }

            let root_mut = &mut *root.as_ptr();
            root_mut.mark_not_in_heap();
        }

        Some(root)
    }

    /// Remove a specific node from the heap.
    ///
    /// Returns true if the node was in the heap and was removed.
    ///
    /// # Safety
    /// The caller must ensure the node pointer is valid.
    pub unsafe fn erase(&mut self, node: NonNull<N>) -> bool {
        unsafe {
            let node_ref = node.as_ref();

            // Check if node is in the heap
            if !node_ref.is_in_heap() && self.root != Some(node) {
                return false;
            }

            if self.root == Some(node) {
                self.pop_front();
                return true;
            }

            // Remove from parent
            if let Some(parent) = node_ref.parent() {
                let parent_mut = &mut *parent.as_ptr();
                if parent_mut.left() == Some(node) {
                    parent_mut.set_left(node_ref.left());
                } else {
                    parent_mut.set_right(node_ref.left());
                }

                // Reattach children
                if let Some(left) = node_ref.left() {
                    (*left.as_ptr()).set_parent(Some(parent));
                }
                if let Some(right) = node_ref.right() {
                    // Attach right child somewhere
                    if let Some(left) = node_ref.left() {
                        (*left.as_ptr()).set_right(Some(right));
                        (*right.as_ptr()).set_parent(Some(left));
                    } else {
                        parent_mut.set_right(Some(right));
                        (*right.as_ptr()).set_parent(Some(parent));
                    }
                }
            }

            self.size -= 1;
            let node_mut = &mut *node.as_ptr();
            node_mut.mark_not_in_heap();
            true
        }
    }
}

/// Compute the smallest power of 2 greater than or equal to n.
#[allow(dead_code)]
fn bit_ceil(n: usize) -> usize {
    if n <= 1 {
        return 1;
    }
    let leading_zeros = (n - 1).leading_zeros();
    1usize << (usize::BITS - leading_zeros)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct TestNode {
        key: i32,
        parent: Cell<Option<NonNull<TestNode>>>,
        left: Cell<Option<NonNull<TestNode>>>,
        right: Cell<Option<NonNull<TestNode>>>,
        in_heap: Cell<bool>,
    }

    impl TestNode {
        fn new(key: i32) -> Self {
            Self {
                key,
                parent: Cell::new(None),
                left: Cell::new(None),
                right: Cell::new(None),
                in_heap: Cell::new(true),
            }
        }
    }

    impl IntrusiveHeapNode for TestNode {
        type Key = i32;

        fn key(&self) -> &Self::Key {
            &self.key
        }

        fn parent(&self) -> Option<NonNull<Self>> {
            self.parent.get()
        }

        fn set_parent(&mut self, parent: Option<NonNull<Self>>) {
            self.parent.set(parent);
        }

        fn left(&self) -> Option<NonNull<Self>> {
            self.left.get()
        }

        fn set_left(&mut self, left: Option<NonNull<Self>>) {
            self.left.set(left);
        }

        fn right(&self) -> Option<NonNull<Self>> {
            self.right.get()
        }

        fn set_right(&mut self, right: Option<NonNull<Self>>) {
            self.right.set(right);
        }

        fn is_in_heap(&self) -> bool {
            self.in_heap.get() || self.parent.get().is_some()
        }

        fn mark_not_in_heap(&mut self) {
            self.in_heap.set(false);
            self.parent.set(None);
        }
    }

    #[test]
    fn test_empty_heap() {
        let heap: IntrusiveHeap<TestNode> = IntrusiveHeap::new();
        assert!(heap.is_empty());
        assert_eq!(heap.len(), 0);
        assert!(heap.front().is_none());
    }

    #[test]
    fn test_single_insert() {
        let mut heap: IntrusiveHeap<TestNode> = IntrusiveHeap::new();
        let mut node = TestNode::new(42);

        unsafe {
            heap.insert(NonNull::new(&mut node).unwrap());
        }

        assert!(!heap.is_empty());
        assert_eq!(heap.len(), 1);
        assert!(heap.front().is_some());

        unsafe {
            assert_eq!(heap.front().unwrap().as_ref().key, 42);
        }
    }

    #[test]
    fn test_multiple_insert() {
        let mut heap: IntrusiveHeap<TestNode> = IntrusiveHeap::new();
        let mut nodes = [
            TestNode::new(3),
            TestNode::new(1),
            TestNode::new(2),
        ];

        unsafe {
            for node in &mut nodes {
                heap.insert(NonNull::new(node).unwrap());
            }
        }

        assert_eq!(heap.len(), 3);

        // The minimum should be at front
        unsafe {
            let front = heap.front().unwrap();
            assert_eq!(front.as_ref().key, 1);
        }
    }

    #[test]
    fn test_bit_ceil() {
        assert_eq!(bit_ceil(1), 1);
        assert_eq!(bit_ceil(2), 2);
        assert_eq!(bit_ceil(3), 4);
        assert_eq!(bit_ceil(4), 4);
        assert_eq!(bit_ceil(5), 8);
        assert_eq!(bit_ceil(7), 8);
        assert_eq!(bit_ceil(8), 8);
        assert_eq!(bit_ceil(9), 16);
    }
}
