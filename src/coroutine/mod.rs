use std::collections::VecDeque;
use std::pin::Pin;

use bevy::ecs::component::ComponentId;
use bevy::prelude::Entity;
use bevy::prelude::Resource;
use bevy::prelude::Timer;
use bevy::prelude::World;
use bevy::utils::synccell::SyncCell;
use tinyset::SetUsize;

use self::observable::ObservableId;

pub mod coro_param;
pub mod duration;
pub mod function_coroutine;
pub mod observable;
pub mod on;
pub mod par_and;
pub mod par_or;
mod waker;
mod when;

/// A coroutine that can be added to an [`Entity`](bevy::prelude::Entity)
///
pub trait Coroutine: Send + 'static {
    /// Resume execution of this coroutine.
    ///
    /// It will returns [`CoroutineResult::Yield`] if it can be resumed once again
    /// or [`CoroutineResult::Done`] if it is done executing. Note that resuming a coroutine which
    /// has terminate execution will panic.
    fn resume(self: Pin<&mut Self>, world: &mut World) -> CoroutineResult<WaitingReason, ()>;

    /// Return the metadata of this coroutine, including - ComponentAccess
    fn meta(&self) -> &CoroMeta;
}

/// A shared list of modified values (observable), to easily notify the apropriate observers.
#[derive(Resource, Default)]
pub(crate) struct CoroWrites(pub VecDeque<(Entity, ComponentId)>);

/// A heap allocated Coroutine
pub(crate) type CoroObject = SyncCell<Pin<Box<dyn Coroutine>>>;

/// Metadata of a coroutine
///
/// This includes
/// - Which components are read and written to
#[derive(Clone)]
pub struct CoroMeta {
    pub(crate) this_reads: SetUsize,
    pub(crate) this_writes: SetUsize,
    pub(crate) owner: Entity,
}

impl CoroMeta {
    pub fn new(owner: Entity) -> Self {
        Self {
            owner,
            this_reads: SetUsize::default(),
            this_writes: SetUsize::default(),
        }
    }

    /// Check if the values captured by the coroutine still exists.
    pub(crate) fn is_valid(&self, world: &World) -> bool {
        if !world.entities().contains(self.owner) {
            return false;
        }

        let e = world.entity(self.owner);
        for r in self.this_reads.iter() {
            if !e.contains_id(ComponentId::new(r)) {
                return false;
            }
        }
        for w in self.this_writes.iter() {
            if !e.contains_id(ComponentId::new(w)) {
                return false;
            }
        }

        true
    }
}

/// The result after resuming a `Coroutine`. Either it yields an intermediate value, or return the
/// final result.
pub enum CoroutineResult<Y, R> {
    Yield(Y),
    Done(R),
}

pub trait IntoCoroutine<Marker> {
    type Coroutine: Coroutine;

    fn into_coroutine(self, owner: Entity) -> Self::Coroutine;
}

/// Any type that can be turned into a [`Coroutine`], given an access to the [`World`], to run
/// arbitrary setup logic, such as registering [`Component`].
pub trait UninitCoroutine<Marker> {
    type Coroutine: Coroutine;

    /// Initialize the Coroutine, or return None if it is invalid.
    ///
    /// The world should be used to initialize any accesses, such as registering any
    /// [`Component`](bevy::prelude::Component).
    fn init(self, owner: Entity, world: &mut World) -> Option<Self::Coroutine>;
}

/// A [`Coroutine`] can only yield one of these messages Used by the [`Executor`] to know when to
/// resume a coroutine.
pub enum WaitingReason {
    Tick,
    Duration(Timer),
    Changed(ObservableId),
    ParOr { coroutines: Vec<CoroObject> },
    ParAnd { coroutines: Vec<CoroObject> },
}

// TODO put that somewhere else
#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum CoroState {
    Halted,
    Running,
}
