use std::pin::Pin;

use bevy::prelude::World;
use bevy::time::Timer;
use bevy::utils::synccell::SyncCell;
use tinyset::SetU64;

use self::id_alloc::Id;

mod all;
mod executor;
mod first;
mod function_coroutine;
mod handle;
mod id_alloc;
mod scope;
mod tick;
mod waker;

// THINGS MISSING:
// Dropping a handle should drop the coroutine
// Dropping a scope should drop the local entities
// Commands
// SIGNALS !!!

/// A coroutine is a form of state machine. It can get resumed, and returns on which condition it
/// should be resumed again.
pub trait Coroutine: Send + 'static {
    /// Resume execution of this coroutine.
    ///
    fn resume(self: Pin<&mut Self>, world: &mut World) -> CoroutineResult;

    /// Return true, if this coroutine is still valid. If it is not, it should be despawned.
    /// Should be called before [`resume`], to avoid any panic.
    fn is_valid(&self, world: &World) -> bool;
}

pub struct CoroutineResult {
    result: CoroutineStatus,
    new_coro: Vec<NewCoroutine>,
    // TODO: triggered_signals?
}

pub struct NewCoroutine {
    id: Id,
    coroutine: HeapCoro,
    coro_type: CoroType,
    should_start_now: bool,
}

pub enum CoroType {
    Local,
    Handled,
    Background,
}

/// The status of the [`Coroutine`] after being resumed. If it is [`CoroutineStatus::Yield`], then
/// the coroutine should be resumed again once the condition is fullfiled. If it is [`Done`] then
/// the coroutine has finish execution and should not be resumed again, doing so will panic.
pub enum CoroutineStatus {
    Yield(WaitingReason),
    Done,
}

/// The condition for a [`Coroutine`] to be resumed.
pub enum WaitingReason {
    /// Get resumed after one tick
    Tick,
    /// Get resumed once the duration is reached
    Duration(Timer),
    /// Get resumed once any of the coroutine has terminate
    First(SetU64),
    /// Get resumed once all coroutines have terminate
    All(SetU64),
    /// Never get resumed, and gets cleanup instead
    Cancel,
}

/// A heap allocated [`Coroutine`]
/// It is pinned since most coroutine are implemented using [`Future`]. [`SyncCell`] is used to
/// make them [`Sync`] while being only [`Send`].
type HeapCoro = SyncCell<Pin<Box<dyn Coroutine>>>;
