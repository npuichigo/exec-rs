//! Intrusive queue implementations.
//!
//! Provides both single-threaded and atomic (MPSC) intrusive queues.
//! Intrusive means tasks contain their own next pointer, avoiding heap allocation
//! for queue nodes.

use std::cell::Cell;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};
use wait_on_address::AtomicWait;

/// Trait for items that can be stored in an intrusive queue.
///
/// Items must provide access to a `next` pointer field.
///
/// # Safety
///
/// Implementations must ensure:
/// - The `next` pointer is only modified by the queue
/// - Items are not in multiple queues simultaneously
pub unsafe trait IntrusiveNode {
    /// Get a reference to the next pointer.
    fn next(&self) -> &Cell<Option<NonNull<Self>>>;
}

/// A single-threaded intrusive queue (FIFO).
///
/// This queue does not allocate memory for queue nodes - items contain
/// their own `next` pointer.
pub struct IntrusiveQueue<T: IntrusiveNode> {
    head: Option<NonNull<T>>,
    tail: Option<NonNull<T>>,
}

impl<T: IntrusiveNode> Default for IntrusiveQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: IntrusiveNode> IntrusiveQueue<T> {
    /// Create a new empty queue.
    pub const fn new() -> Self {
        Self {
            head: None,
            tail: None,
        }
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    /// Push an item to the back of the queue.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `item` is a valid pointer to a pinned item
    /// - `item` is not already in any queue
    pub unsafe fn push_back(&mut self, item: NonNull<T>) {
        // SAFETY: Caller guarantees item is valid
        unsafe {
            // Set next to None
            item.as_ref().next().set(None);

            if let Some(tail) = self.tail {
                // Link old tail to new item
                tail.as_ref().next().set(Some(item));
            } else {
                // Queue was empty
                self.head = Some(item);
            }
            self.tail = Some(item);
        }
    }

    /// Push an item to the front of the queue.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `item` is a valid pointer to a pinned item
    /// - `item` is not already in any queue
    pub unsafe fn push_front(&mut self, item: NonNull<T>) {
        // SAFETY: Caller guarantees item is valid
        unsafe {
            item.as_ref().next().set(self.head);
            self.head = Some(item);
            if self.tail.is_none() {
                self.tail = Some(item);
            }
        }
    }

    /// Pop an item from the front of the queue.
    ///
    /// Returns `None` if the queue is empty.
    pub fn pop_front(&mut self) -> Option<NonNull<T>> {
        let head = self.head?;

        // SAFETY: head is valid because it's in our queue
        let next = unsafe { head.as_ref().next().get() };
        self.head = next;

        if self.head.is_none() {
            self.tail = None;
        }

        Some(head)
    }

    /// Append another queue to the end of this queue.
    ///
    /// The other queue is left empty after this operation.
    pub fn append(&mut self, other: &mut IntrusiveQueue<T>) {
        if other.is_empty() {
            return;
        }

        if let Some(tail) = self.tail {
            // Link our tail to other's head
            // SAFETY: tail is valid because it's in our queue
            unsafe {
                tail.as_ref().next().set(other.head);
            }
        } else {
            // Our queue was empty
            self.head = other.head;
        }
        self.tail = other.tail;

        // Clear the other queue
        other.head = None;
        other.tail = None;
    }

    /// Create a queue from a reversed linked list.
    ///
    /// The input list has elements in LIFO order (most recent first).
    /// This reverses them to FIFO order.
    ///
    /// # Safety
    ///
    /// The caller must ensure all nodes in the list are valid.
    pub unsafe fn from_reversed(mut list: Option<NonNull<T>>) -> Self {
        let mut new_head: Option<NonNull<T>> = None;
        let new_tail = list; // First element becomes the tail after reversal

        while let Some(node) = list {
            // SAFETY: Caller guarantees all nodes are valid
            unsafe {
                let next = node.as_ref().next().get();
                node.as_ref().next().set(new_head);
                new_head = Some(node);
                list = next;
            }
        }

        Self {
            head: new_head,
            tail: new_tail,
        }
    }

    /// Clear the queue without running destructors.
    pub fn clear(&mut self) {
        self.head = None;
        self.tail = None;
    }

    /// Iterate over items in the queue (non-consuming).
    pub fn iter(&self) -> IntrusiveQueueIter<'_, T> {
        IntrusiveQueueIter {
            current: self.head,
            _marker: std::marker::PhantomData,
        }
    }
}

/// Iterator over items in an intrusive queue.
pub struct IntrusiveQueueIter<'a, T: IntrusiveNode> {
    current: Option<NonNull<T>>,
    _marker: std::marker::PhantomData<&'a T>,
}

impl<'a, T: IntrusiveNode> Iterator for IntrusiveQueueIter<'a, T> {
    type Item = NonNull<T>;

    fn next(&mut self) -> Option<Self::Item> {
        let current = self.current?;
        // SAFETY: current is valid because it's in the queue
        self.current = unsafe { current.as_ref().next().get() };
        Some(current)
    }
}

/// An atomic MPSC (Multiple Producer, Single Consumer) intrusive queue.
///
/// This queue is lock-free for producers and supports bulk dequeue.
/// Items are pushed in LIFO order but can be reversed on pop_all.
/// Uses atomic wait/notify for efficient blocking (like C++20 atomic::wait).
pub struct AtomicIntrusiveQueue<T> {
    /// Head pointer stored as u64 for atomic wait/notify support.
    /// Using u64 allows us to use wait_on_address's AtomicWait trait.
    head: AtomicU64,
    _marker: std::marker::PhantomData<*mut T>,
}

impl<T> Default for AtomicIntrusiveQueue<T> {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: The queue is designed for multi-threaded access
unsafe impl<T: Send> Send for AtomicIntrusiveQueue<T> {}
unsafe impl<T: Send> Sync for AtomicIntrusiveQueue<T> {}

impl<T> AtomicIntrusiveQueue<T> {
    /// Create a new empty atomic queue.
    pub const fn new() -> Self {
        Self {
            head: AtomicU64::new(0), // null pointer
            _marker: std::marker::PhantomData,
        }
    }

    /// Check if the queue appears to be empty.
    ///
    /// Note: This may be immediately stale in a concurrent context.
    pub fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire) == 0
    }
}

impl<T: IntrusiveNode> AtomicIntrusiveQueue<T> {
    /// Push an item to the queue (lock-free).
    ///
    /// Returns `true` if the queue was empty before this push
    /// (useful for waking a consumer).
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    /// - `item` is a valid pointer to a pinned item
    /// - `item` is not already in any queue
    pub unsafe fn push(&self, item: NonNull<T>) -> bool {
        let item_u64 = item.as_ptr() as u64;
        let mut old_head = self.head.load(Ordering::Relaxed);

        loop {
            // Set item's next to current head
            // SAFETY: Caller guarantees item is valid
            unsafe {
                let old_ptr = old_head as usize as *mut T;
                item.as_ref().next().set(NonNull::new(old_ptr));
            }

            // Try to CAS head to point to our item
            match self.head.compare_exchange_weak(
                old_head,
                item_u64,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => old_head = current,
            }
        }

        // Return true if the queue was empty
        let was_empty = old_head == 0;

        // If the queue was empty, wake a waiting consumer.
        // This matches stdexec's pattern exactly.
        if was_empty {
            self.head.notify_one();
        }

        was_empty
    }

    /// Wait until the queue has at least one item.
    ///
    /// This uses atomic wait on the head pointer directly, which is race-free:
    /// the check and wait are atomic (like C++20 atomic::wait).
    pub fn wait_for_item(&self) {
        // Wait while head is 0 (null pointer = empty queue)
        self.head.wait(0);
    }

    /// Pop all items from the queue atomically.
    ///
    /// Returns an intrusive queue with items in FIFO order
    /// (reversed from the internal LIFO storage).
    pub fn pop_all(&self) -> IntrusiveQueue<T> {
        let list_u64 = self.head.swap(0, Ordering::Acquire);
        let list_ptr = list_u64 as usize as *mut T;

        // SAFETY: All items in the list were validly pushed
        unsafe { IntrusiveQueue::from_reversed(NonNull::new(list_ptr)) }
    }

    /// Pop all items without reversing (LIFO order).
    ///
    /// Returns the raw linked list head pointer.
    pub fn pop_all_lifo(&self) -> Option<NonNull<T>> {
        let u64_val = self.head.swap(0, Ordering::Acquire);
        NonNull::new(u64_val as usize as *mut T)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test node type
    struct TestNode {
        value: i32,
        next: Cell<Option<NonNull<TestNode>>>,
    }

    impl TestNode {
        fn new(value: i32) -> Self {
            Self {
                value,
                next: Cell::new(None),
            }
        }
    }

    unsafe impl IntrusiveNode for TestNode {
        fn next(&self) -> &Cell<Option<NonNull<Self>>> {
            &self.next
        }
    }

    #[test]
    fn test_intrusive_queue_basic() {
        let mut queue = IntrusiveQueue::<TestNode>::new();
        assert!(queue.is_empty());

        let mut node1 = TestNode::new(1);
        let mut node2 = TestNode::new(2);
        let mut node3 = TestNode::new(3);

        unsafe {
            queue.push_back(NonNull::new_unchecked(&mut node1));
            queue.push_back(NonNull::new_unchecked(&mut node2));
            queue.push_back(NonNull::new_unchecked(&mut node3));
        }

        assert!(!queue.is_empty());

        // Should get them back in FIFO order
        let p1 = queue.pop_front().unwrap();
        assert_eq!(unsafe { p1.as_ref().value }, 1);

        let p2 = queue.pop_front().unwrap();
        assert_eq!(unsafe { p2.as_ref().value }, 2);

        let p3 = queue.pop_front().unwrap();
        assert_eq!(unsafe { p3.as_ref().value }, 3);

        assert!(queue.is_empty());
        assert!(queue.pop_front().is_none());
    }

    #[test]
    fn test_intrusive_queue_push_front() {
        let mut queue = IntrusiveQueue::<TestNode>::new();

        let mut node1 = TestNode::new(1);
        let mut node2 = TestNode::new(2);

        unsafe {
            queue.push_back(NonNull::new_unchecked(&mut node1));
            queue.push_front(NonNull::new_unchecked(&mut node2));
        }

        // node2 was pushed to front, so it comes out first
        let p1 = queue.pop_front().unwrap();
        assert_eq!(unsafe { p1.as_ref().value }, 2);

        let p2 = queue.pop_front().unwrap();
        assert_eq!(unsafe { p2.as_ref().value }, 1);
    }

    #[test]
    fn test_atomic_queue_basic() {
        let queue = AtomicIntrusiveQueue::<TestNode>::new();
        assert!(queue.is_empty());

        let mut node1 = TestNode::new(1);
        let mut node2 = TestNode::new(2);
        let mut node3 = TestNode::new(3);

        unsafe {
            let was_empty1 = queue.push(NonNull::new_unchecked(&mut node1));
            assert!(was_empty1); // First push to empty queue

            let was_empty2 = queue.push(NonNull::new_unchecked(&mut node2));
            assert!(!was_empty2); // Queue was not empty

            let was_empty3 = queue.push(NonNull::new_unchecked(&mut node3));
            assert!(!was_empty3);
        }

        assert!(!queue.is_empty());

        // pop_all returns FIFO order
        let mut result = queue.pop_all();
        assert!(queue.is_empty());

        let p1 = result.pop_front().unwrap();
        assert_eq!(unsafe { p1.as_ref().value }, 1);

        let p2 = result.pop_front().unwrap();
        assert_eq!(unsafe { p2.as_ref().value }, 2);

        let p3 = result.pop_front().unwrap();
        assert_eq!(unsafe { p3.as_ref().value }, 3);

        assert!(result.is_empty());
    }

    #[test]
    fn test_atomic_queue_concurrent() {
        use std::sync::Arc;
        use std::thread;

        // We need to use boxed nodes for thread safety
        struct BoxedNode {
            value: i32,
            next: Cell<Option<NonNull<BoxedNode>>>,
        }

        // SAFETY: BoxedNode only contains value and Cell<Option<NonNull>>
        // The Cell is only accessed through the intrusive queue's atomic operations
        unsafe impl Send for BoxedNode {}
        unsafe impl Sync for BoxedNode {}

        unsafe impl IntrusiveNode for BoxedNode {
            fn next(&self) -> &Cell<Option<NonNull<Self>>> {
                &self.next
            }
        }

        let queue = Arc::new(AtomicIntrusiveQueue::<BoxedNode>::new());
        let num_threads = 4;
        let items_per_thread = 100;

        let mut handles = vec![];

        for t in 0..num_threads {
            let queue = queue.clone();

            handles.push(thread::spawn(move || {
                for i in 0..items_per_thread {
                    // Allocate node inside the thread
                    let node = Box::into_raw(Box::new(BoxedNode {
                        value: (t * items_per_thread + i) as i32,
                        next: Cell::new(None),
                    }));
                    unsafe {
                        queue.push(NonNull::new_unchecked(node));
                    }
                }
            }));
        }

        for handle in handles {
            handle.join().unwrap();
        }

        // Pop all and count
        let mut result = queue.pop_all();
        let mut count = 0;
        while let Some(node_ptr) = result.pop_front() {
            count += 1;
            // Free the node
            unsafe {
                drop(Box::from_raw(node_ptr.as_ptr()));
            }
        }

        assert_eq!(count, num_threads * items_per_thread);
    }
}
