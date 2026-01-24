//! StartsOn adaptor - start a sender on a specific scheduler.

use crate::traits::{Scheduler, Sender};

/// Start a sender on a specific scheduler.
///
/// This is similar to `on`, but the emphasis is on where execution starts.
pub fn starts_on<S, Sch>(sender: S, scheduler: Sch) -> StartsOn<S, Sch> {
    StartsOn::new(sender, scheduler)
}

/// A sender that starts on a specific scheduler.
#[derive(Debug, Clone)]
pub struct StartsOn<S, Sch> {
    sender: S,
    scheduler: Sch,
}

impl<S, Sch> StartsOn<S, Sch> {
    /// Create a new StartsOn adaptor.
    pub fn new(sender: S, scheduler: Sch) -> Self {
        Self { sender, scheduler }
    }

    /// Get the inner sender.
    pub fn into_inner(self) -> S {
        self.sender
    }

    /// Get a reference to the scheduler.
    pub fn scheduler(&self) -> &Sch {
        &self.scheduler
    }
}

impl<S: Sender, Sch: Scheduler> Sender for StartsOn<S, Sch> {
    type Completions = S::Completions;
}

// StartsOn is implemented in terms of On
impl<S, Sch, R> crate::traits::SenderTo<R> for StartsOn<S, Sch>
where
    S: Sender + crate::traits::SenderTo<R>,
    Sch: Scheduler,
    Sch::Sender: crate::traits::SenderTo<crate::adaptors::OnScheduleReceiver<S, R>>,
    R: crate::traits::ReceiverOf<<S::Completions as crate::traits::CompletionSignatures>::Value>
        + crate::traits::ReceiverOfError<<S::Completions as crate::traits::CompletionSignatures>::Error>
        + crate::traits::ReceiverOfStopped,
{
    type Operation = crate::adaptors::OnOperation<Sch, S, R>;

    fn connect(self, receiver: R) -> Self::Operation {
        crate::adaptors::on(self.scheduler, self.sender).connect(receiver)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consumers::sync_wait;
    use crate::factories::just;
    use crate::schedulers::InlineScheduler;

    #[test]
    fn test_starts_on() {
        let result = sync_wait(starts_on(just(42), InlineScheduler)).unwrap();
        assert_eq!(result, 42);
    }
}
