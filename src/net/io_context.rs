//! I/O Context - the async I/O runtime using mio.
//!
//! The `IoContext` manages the event loop and coordinates async I/O operations.
//! It uses mio's `Poll` for efficient event notification.

use crate::traits::{
    CompletionSignatures, OperationState, ReceiverOf, Scheduler, Sender, SenderTo,
};
use crate::util::{AtomicIntrusiveQueue, IntrusiveNode};
use mio::{Events, Interest, Poll, Registry, Token, Waker};
use std::cell::{Cell, RefCell, UnsafeCell};
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Unique identifier for sockets within an IoContext.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SocketId(pub(crate) usize);

/// Event types for I/O readiness.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IoEvent {
    /// Socket is readable.
    Readable,
    /// Socket is writable.
    Writable,
    /// Socket is both readable and writable.
    ReadWrite,
}

impl IoEvent {
    /// Convert to mio Interest.
    pub fn to_interest(self) -> Interest {
        match self {
            IoEvent::Readable => Interest::READABLE,
            IoEvent::Writable => Interest::WRITABLE,
            IoEvent::ReadWrite => Interest::READABLE | Interest::WRITABLE,
        }
    }

    /// Check if readable.
    pub fn is_readable(self) -> bool {
        matches!(self, IoEvent::Readable | IoEvent::ReadWrite)
    }

    /// Check if writable.
    pub fn is_writable(self) -> bool {
        matches!(self, IoEvent::Writable | IoEvent::ReadWrite)
    }
}

/// Completion callback for I/O operations.
/// Completion callback for I/O operations.
///
/// Note: Send is not required because all completions run on the IoContext thread.
/// The task_queue is for cross-thread task submission, not for I/O completions.
pub(crate) type IoCompletion = Box<dyn FnOnce(io::Result<IoEvent>)>;

/// A pending I/O operation waiting for readiness.
pub(crate) struct PendingIo {
    /// The interest we're waiting for.
    pub interest: Interest,
    /// The completion callback.
    pub completion: IoCompletion,
}

/// Task that can be queued for execution.
pub(crate) struct IoTask {
    /// Function to execute.
    execute_fn: fn(*mut IoTask),
    /// Next pointer for intrusive queue.
    next: Cell<Option<NonNull<IoTask>>>,
}

unsafe impl IntrusiveNode for IoTask {
    fn next(&self) -> &Cell<Option<NonNull<Self>>> {
        &self.next
    }
}

unsafe impl Send for IoTask {}
unsafe impl Sync for IoTask {}

impl IoTask {
    /// Create a new task.
    pub fn new(execute_fn: fn(*mut IoTask)) -> Self {
        Self {
            execute_fn,
            next: Cell::new(None),
        }
    }

    /// Execute the task.
    pub unsafe fn execute(&self, ptr: *mut IoTask) {
        (self.execute_fn)(ptr);
    }
}

/// Internal state for a registered socket.
struct SocketState {
    /// The mio source (stored as raw fd for flexibility).
    #[cfg(unix)]
    fd: std::os::unix::io::RawFd,
    /// Whether the socket is registered with poll.
    registered: bool,
    /// Current interest.
    interest: Interest,
}

/// Shared state for the IoContext.
pub(crate) struct IoContextInner {
    /// mio Poll instance.
    poll: RefCell<Poll>,
    /// Waker for cross-thread notification.
    waker: Arc<Waker>,
    /// Next token value.
    next_token: AtomicUsize,
    /// Socket states indexed by token.
    sockets: RefCell<HashMap<Token, SocketState>>,
    /// Pending I/O operations indexed by token.
    pending_io: RefCell<HashMap<Token, PendingIo>>,
    /// Task queue for deferred execution.
    task_queue: AtomicIntrusiveQueue<IoTask>,
    /// Whether the context is running.
    running: AtomicBool,
    /// Whether stop has been requested.
    stop_requested: AtomicBool,
}

// SAFETY: IoContextInner is designed for single-threaded use with cross-thread wakeup.
// The Poll, sockets, and pending_io are only accessed from the run thread.
// The task_queue and waker are thread-safe.
unsafe impl Send for IoContextInner {}
unsafe impl Sync for IoContextInner {}

/// The async I/O context.
///
/// This is the central hub for async I/O operations. It manages the mio event
/// loop and coordinates sender/receiver-based async operations.
pub struct IoContext {
    pub(crate) inner: Arc<IoContextInner>,
}

impl IoContext {
    /// Create a new I/O context.
    pub fn new() -> io::Result<Self> {
        let poll = Poll::new()?;
        let waker = Arc::new(Waker::new(poll.registry(), Token(0))?);

        Ok(Self {
            inner: Arc::new(IoContextInner {
                poll: RefCell::new(poll),
                waker,
                next_token: AtomicUsize::new(1), // 0 is reserved for waker
                sockets: RefCell::new(HashMap::new()),
                pending_io: RefCell::new(HashMap::new()),
                task_queue: AtomicIntrusiveQueue::new(),
                running: AtomicBool::new(false),
                stop_requested: AtomicBool::new(false),
            }),
        })
    }

    /// Get the poll registry for socket registration.
    pub(crate) fn registry(&self) -> impl std::ops::Deref<Target = Registry> + '_ {
        std::cell::Ref::map(self.inner.poll.borrow(), |p| p.registry())
    }

    /// Allocate a new token for a socket.
    pub(crate) fn allocate_token(&self) -> Token {
        Token(self.inner.next_token.fetch_add(1, Ordering::Relaxed))
    }

    /// Register a socket with the context.
    #[cfg(unix)]
    pub(crate) fn register_socket(
        &self,
        token: Token,
        fd: std::os::unix::io::RawFd,
        interest: Interest,
    ) -> io::Result<SocketId> {
        use mio::unix::SourceFd;

        let mut source_fd = SourceFd(&fd);
        self.inner
            .poll
            .borrow()
            .registry()
            .register(&mut source_fd, token, interest)?;

        self.inner.sockets.borrow_mut().insert(
            token,
            SocketState {
                fd,
                registered: true,
                interest,
            },
        );

        Ok(SocketId(token.0))
    }

    /// Deregister a socket from the context.
    #[cfg(unix)]
    pub(crate) fn deregister_socket(&self, token: Token) -> io::Result<()> {
        use mio::unix::SourceFd;

        if let Some(state) = self.inner.sockets.borrow_mut().remove(&token) {
            if state.registered {
                let mut source_fd = SourceFd(&state.fd);
                self.inner
                    .poll
                    .borrow()
                    .registry()
                    .deregister(&mut source_fd)?;
            }
        }

        // Also remove any pending operations.
        self.inner.pending_io.borrow_mut().remove(&token);

        Ok(())
    }

    /// Update interest for a socket.
    #[cfg(unix)]
    pub(crate) fn update_interest(&self, token: Token, interest: Interest) -> io::Result<()> {
        use mio::unix::SourceFd;

        if let Some(state) = self.inner.sockets.borrow_mut().get_mut(&token) {
            if state.interest != interest {
                let mut source_fd = SourceFd(&state.fd);
                self.inner
                    .poll
                    .borrow()
                    .registry()
                    .reregister(&mut source_fd, token, interest)?;
                state.interest = interest;
            }
        }

        Ok(())
    }

    /// Submit an I/O operation to wait for readiness.
    pub(crate) fn submit_io(&self, token: Token, interest: Interest, completion: IoCompletion) {
        // Update socket interest if needed.
        self.update_interest(token, interest).ok();

        // Store the pending operation.
        self.inner
            .pending_io
            .borrow_mut()
            .insert(token, PendingIo { interest, completion });
    }

    /// Get a scheduler for this context.
    pub fn get_scheduler(&self) -> IoContextScheduler {
        IoContextScheduler {
            inner: self.inner.clone(),
        }
    }

    /// Run the event loop until stop is requested.
    ///
    /// Returns the number of events processed.
    pub fn run(&self) -> io::Result<usize> {
        self.inner.running.store(true, Ordering::Release);
        self.inner.stop_requested.store(false, Ordering::Release);

        let mut total_events = 0;
        let mut events = Events::with_capacity(1024);

        while !self.inner.stop_requested.load(Ordering::Acquire) {
            total_events += self.run_one_inner(&mut events)?;
        }

        self.inner.running.store(false, Ordering::Release);
        Ok(total_events)
    }

    /// Run a single iteration of the event loop.
    ///
    /// Returns the number of events processed.
    pub fn run_one(&self) -> io::Result<usize> {
        let mut events = Events::with_capacity(64);
        self.run_one_inner(&mut events)
    }

    fn run_one_inner(&self, events: &mut Events) -> io::Result<usize> {
        // Process any queued tasks first.
        self.process_tasks();

        // Poll for events.
        self.inner
            .poll
            .borrow_mut()
            .poll(events, Some(Duration::from_millis(100)))?;

        let mut count = 0;

        // Collect completions to run - we must release the borrow before calling them
        // because completions may call submit_io which borrows pending_io.
        let mut completions_to_run = Vec::new();

        for event in events.iter() {
            let token = event.token();

            // Skip waker token.
            if token == Token(0) {
                continue;
            }

            // Determine what happened.
            let io_event = if event.is_readable() && event.is_writable() {
                IoEvent::ReadWrite
            } else if event.is_readable() {
                IoEvent::Readable
            } else if event.is_writable() {
                IoEvent::Writable
            } else {
                continue;
            };

            // Remove the pending operation (borrow is released after this statement).
            if let Some(pending) = self.inner.pending_io.borrow_mut().remove(&token) {
                completions_to_run.push((pending.completion, io_event));
            }
        }

        // Now run completions without holding any borrows.
        for (completion, io_event) in completions_to_run {
            completion(Ok(io_event));
            count += 1;
        }

        // Process tasks that may have been queued during event handling.
        self.process_tasks();

        Ok(count)
    }

    fn process_tasks(&self) {
        let mut queue = self.inner.task_queue.pop_all();
        while let Some(task_ptr) = queue.pop_front() {
            unsafe {
                let task = task_ptr.as_ref();
                task.execute(task_ptr.as_ptr());
            }
        }
    }

    /// Request the event loop to stop.
    pub fn stop(&self) {
        self.inner.stop_requested.store(true, Ordering::Release);
        // Wake up the poll if it's waiting.
        self.inner.waker.wake().ok();
    }

    /// Check if the context is currently running.
    pub fn is_running(&self) -> bool {
        self.inner.running.load(Ordering::Acquire)
    }

    /// Check if stop has been requested.
    pub fn is_stop_requested(&self) -> bool {
        self.inner.stop_requested.load(Ordering::Acquire)
    }

    /// Wake up the event loop from another thread.
    pub fn wake(&self) -> io::Result<()> {
        self.inner.waker.wake()
    }

    /// Spawn a sender to run on this IoContext.
    ///
    /// The sender is started immediately but runs asynchronously.
    /// Call `run()` to drive the event loop and process completions.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let ctx = IoContext::new()?;
    /// let listener = TcpListener::bind(&ctx, "127.0.0.1:12345")?;
    ///
    /// // Build the server as a sender graph
    /// let server = repeat_effect_until(|| {
    ///     async_accept(&listener, &ctx)
    ///         .let_value(|(stream, addr)| {
    ///             // Handle client...
    ///             just(false) // Continue accepting
    ///         })
    /// });
    ///
    /// // Spawn and run
    /// ctx.spawn(server);
    /// ctx.run()?;
    /// ```
    pub fn spawn<S, V, E>(&self, sender: S)
    where
        S: crate::traits::Sender<Completions: crate::traits::CompletionSignatures<Value = V, Error = E>>
            + crate::traits::SenderTo<DetachedReceiver<V, E>>,
        V: 'static,
        E: 'static,
    {
        let receiver = DetachedReceiver {
            inner: self.inner.clone(),
            _phantom: std::marker::PhantomData,
        };

        let mut op = Box::pin(sender.connect(receiver));
        op.as_mut().start();

        // Store the operation to keep it alive
        // For now, we leak it - a proper implementation would track spawned operations
        std::mem::forget(op);
    }
}

/// Receiver for detached/spawned senders.
///
/// This receiver discards the result but can optionally stop the context.
pub struct DetachedReceiver<V, E> {
    inner: Arc<IoContextInner>,
    _phantom: std::marker::PhantomData<(V, E)>,
}

impl<V, E> Clone for DetachedReceiver<V, E> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<V, E> crate::traits::Receiver for DetachedReceiver<V, E> {
    type Env = IoContextEnv;

    fn get_env(&self) -> &Self::Env {
        &IoContextEnv
    }
}

impl<V, E> crate::traits::ReceiverOf<V> for DetachedReceiver<V, E> {
    fn set_value(self, _value: V) {
        // Detached - discard value
        // Optionally stop the context when the main sender completes
        self.inner.stop_requested.store(true, Ordering::Release);
    }
}

impl<V, E> crate::traits::ReceiverOfError<E> for DetachedReceiver<V, E> {
    fn set_error(self, _error: E) {
        // Detached - discard error
        self.inner.stop_requested.store(true, Ordering::Release);
    }
}

impl<V, E> crate::traits::ReceiverOfStopped for DetachedReceiver<V, E> {
    fn set_stopped(self) {
        // Detached
        self.inner.stop_requested.store(true, Ordering::Release);
    }
}

/// Environment for IoContext receivers.
pub struct IoContextEnv;

impl crate::traits::Environment for IoContextEnv {
    type StopToken = crate::stop::NeverStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        crate::stop::NeverStopToken
    }
}

impl Default for IoContext {
    fn default() -> Self {
        Self::new().expect("Failed to create IoContext")
    }
}

/// Scheduler for the IoContext.
#[derive(Clone)]
pub struct IoContextScheduler {
    pub(crate) inner: Arc<IoContextInner>,
}

impl std::fmt::Debug for IoContextScheduler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IoContextScheduler").finish_non_exhaustive()
    }
}

impl PartialEq for IoContextScheduler {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for IoContextScheduler {}

/// Completion signatures for IoContext schedule sender.
pub struct IoContextScheduleCompletions;

impl CompletionSignatures for IoContextScheduleCompletions {
    type Value = ();
    type Error = std::convert::Infallible;
    const SENDS_STOPPED: bool = false;
}

/// Sender returned by IoContextScheduler::schedule().
pub struct IoContextScheduleSender {
    inner: Arc<IoContextInner>,
}

impl Sender for IoContextScheduleSender {
    type Completions = IoContextScheduleCompletions;
}

impl<R> SenderTo<R> for IoContextScheduleSender
where
    R: ReceiverOf<()> + Send + 'static,
{
    type Operation = IoContextScheduleOperation<R>;

    fn connect(self, receiver: R) -> Self::Operation {
        IoContextScheduleOperation::new(self.inner, receiver)
    }
}

/// Operation state for IoContext schedule.
#[repr(C)]
pub struct IoContextScheduleOperation<R> {
    task: IoTask,
    inner: Arc<IoContextInner>,
    receiver: UnsafeCell<Option<R>>,
}

unsafe impl<R: Send> Send for IoContextScheduleOperation<R> {}

impl<R: ReceiverOf<()> + Send + 'static> IoContextScheduleOperation<R> {
    fn new(inner: Arc<IoContextInner>, receiver: R) -> Self {
        Self {
            task: IoTask::new(Self::execute_impl),
            inner,
            receiver: UnsafeCell::new(Some(receiver)),
        }
    }

    fn execute_impl(task_ptr: *mut IoTask) {
        let op_ptr = task_ptr as *mut IoContextScheduleOperation<R>;
        unsafe {
            if let Some(receiver) = (*(*op_ptr).receiver.get()).take() {
                receiver.set_value(());
            }
        }
    }
}

impl<R: ReceiverOf<()> + Send + 'static> OperationState for IoContextScheduleOperation<R> {
    fn start(self: Pin<&mut Self>) {
        let this = unsafe { self.get_unchecked_mut() };
        let task_ptr = &mut this.task as *mut IoTask;

        unsafe {
            this.inner.task_queue.push(NonNull::new_unchecked(task_ptr));
        }

        // Wake up the event loop.
        this.inner.waker.wake().ok();
    }
}

impl Scheduler for IoContextScheduler {
    type Sender = IoContextScheduleSender;

    fn schedule(&self) -> Self::Sender {
        IoContextScheduleSender {
            inner: self.inner.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_context_create() {
        let ctx = IoContext::new().unwrap();
        assert!(!ctx.is_running());
    }

    #[test]
    fn test_io_context_scheduler() {
        let ctx = IoContext::new().unwrap();
        let s1 = ctx.get_scheduler();
        let s2 = ctx.get_scheduler();
        assert_eq!(s1, s2);
    }
}
