use bevy::prelude::Entity;
use bevy::{ecs::component::ComponentId, time::Timer};
use tinyset::SetU64;

use crate::{id_alloc::Id, HeapCoro};

/// A newly spawned [`Coroutine`] and how it should be handled by the [`Executor`](executor).
pub struct NewCoroutine {
    pub id: Id,
    pub ran_after: usize,
    pub coroutine: HeapCoro,
    pub is_owned_by: Option<Id>,
    pub should_start_now: bool,
}

/// The msg yield by a [`Coroutine`].
pub struct YieldMsg {
    pub id: Id,
    pub node: usize,
    pub status: CoroStatus,
}

impl YieldMsg {
    pub fn new(id: Id, node: usize, status: CoroStatus) -> Self {
        Self { id, node, status }
    }
}

/// The status of a [`Coroutine`] after being resumed.
pub enum CoroStatus {
    /// Get resumed after one tick
    Tick,
    /// Get resumed once the duration is reached
    Duration(Timer),
    /// Get resumed once any of the coroutine has terminate
    First(SetU64),
    /// Get resumed once all coroutines have terminate
    All(SetU64),
    /// Get resumed once the signal is triggered
    Signal(SignalId),
    /// Has finished execution
    Done,
    /// Never get resumed, and gets cleanup instead
    Cancel,
}

/// The msg notifying that a [`Signal`] was emitted.
pub struct EmitMsg {
    pub id: SignalId,
    pub by: usize,
}

/// The Id of a signal is the concatenation of the component id
/// of the `Signal<S, T>` and the [`Entity`] on which it is defined.
/// Note that signals can also be global, hence have no `owner`.
#[derive(Clone, Copy, Hash, PartialEq, Eq)]
pub struct SignalId {
    pub signal_type: ComponentId,
    pub owner: Option<Entity>,
}
