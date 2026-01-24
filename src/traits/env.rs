//! Environment and query system.
//!
//! The environment provides contextual information to senders and receivers.
//! This includes things like:
//! - Stop tokens for cancellation
//! - Schedulers for continuing work
//! - Custom user-defined queries

use crate::stop::NeverStopToken;

/// An environment that can answer queries.
///
/// Environments provide contextual information to senders during execution.
/// The query mechanism is similar to C++ customization point objects (CPOs).
pub trait Environment: Sized {
    /// The stop token type for this environment.
    type StopToken: crate::stop::StopToken;

    /// Get the stop token from this environment.
    fn get_stop_token(&self) -> Self::StopToken;
}

/// The empty environment that provides default values for all queries.
impl Environment for () {
    type StopToken = NeverStopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        NeverStopToken
    }
}

/// An environment that wraps a stop token.
#[derive(Debug, Clone)]
pub struct StopTokenEnv<T: crate::stop::StopToken> {
    stop_token: T,
}

impl<T: crate::stop::StopToken> StopTokenEnv<T> {
    /// Create a new environment with the given stop token.
    pub fn new(stop_token: T) -> Self {
        Self { stop_token }
    }
}

impl<T: crate::stop::StopToken> Environment for StopTokenEnv<T> {
    type StopToken = T;

    fn get_stop_token(&self) -> Self::StopToken {
        self.stop_token.clone()
    }
}

/// A composed environment that combines multiple environments.
///
/// Queries are resolved by checking the first environment, then falling
/// back to the second if the query is not supported.
#[derive(Debug, Clone)]
pub struct ComposedEnv<E1, E2> {
    primary: E1,
    fallback: E2,
}

impl<E1, E2> ComposedEnv<E1, E2> {
    /// Create a composed environment.
    pub fn new(primary: E1, fallback: E2) -> Self {
        Self { primary, fallback }
    }
}

impl<E1: Environment, E2: Environment> Environment for ComposedEnv<E1, E2> {
    type StopToken = E1::StopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        self.primary.get_stop_token()
    }
}

/// Query trait for getting the scheduler from an environment.
pub trait GetScheduler: Environment {
    /// The scheduler type.
    type Scheduler: crate::traits::Scheduler;

    /// Get the scheduler from this environment.
    fn get_scheduler(&self) -> Self::Scheduler;
}

/// Query trait for getting the delegated scheduler from an environment.
pub trait GetDelegationScheduler: Environment {
    /// The delegation scheduler type.
    type Scheduler: crate::traits::Scheduler;

    /// Get the delegation scheduler.
    ///
    /// This is used by algorithms that need to schedule additional work.
    fn get_delegation_scheduler(&self) -> Self::Scheduler;
}

/// A wrapper environment that adds a scheduler.
#[derive(Debug, Clone)]
pub struct WithScheduler<E, S> {
    env: E,
    scheduler: S,
}

impl<E, S> WithScheduler<E, S> {
    /// Create an environment with an attached scheduler.
    pub fn new(env: E, scheduler: S) -> Self {
        Self { env, scheduler }
    }
}

impl<E: Environment, S: crate::traits::Scheduler> Environment for WithScheduler<E, S> {
    type StopToken = E::StopToken;

    fn get_stop_token(&self) -> Self::StopToken {
        self.env.get_stop_token()
    }
}

impl<E: Environment, S: crate::traits::Scheduler + Clone> GetScheduler for WithScheduler<E, S> {
    type Scheduler = S;

    fn get_scheduler(&self) -> Self::Scheduler {
        self.scheduler.clone()
    }
}

impl<E: Environment, S: crate::traits::Scheduler + Clone> GetDelegationScheduler
    for WithScheduler<E, S>
{
    type Scheduler = S;

    fn get_delegation_scheduler(&self) -> Self::Scheduler {
        self.scheduler.clone()
    }
}
