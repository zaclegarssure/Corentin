use std::pin::Pin;

use bevy::ecs::component::ComponentId;
use bevy::prelude::Entity;
use bevy::prelude::World;
use bevy::time::Timer;
use bevy::utils::synccell::SyncCell;
use bevy::utils::HashMap;
use tinyset::{SetU64, SetUsize};

use self::id_alloc::{Id, Ids};

mod all;
mod commands;
mod coro_param;
mod executor;
mod first;
mod function_coroutine;
mod handle;
mod id_alloc;
mod one_shot;
mod resume;
mod scope;
mod tick;
mod waker;

// THINGS MISSING:
// Dropping a scope should drop the local entities
// Commands
// SIGNALS !!!

/// A coroutine is a form of state machine. It can get resumed, and returns on which condition it
/// should be resumed again.
///
/// Note: Ideally we would use [`UnsafeWorldCell`](bevy::ecs::world::UnsafeWorldCell) here,
/// but we can't due to lifetime issue, so we're back to using raw pointers letssgo
/// This also means that the pointers must be valid before calling any of these functions.
pub trait Coroutine: Send + 'static {
    /// Resume execution of this coroutine.
    fn resume(self: Pin<&mut Self>, world: &mut World, ids: &Ids) -> CoroutineResult;

    /// Return true, if this coroutine is still valid. If it is not, it should be despawned.
    /// Should be called before [`resume`], to avoid any panic.
    fn is_valid(&self, world: &World) -> bool;

    /// Returns this coroutine metadata
    fn meta(&self) -> &CoroMeta;
}

pub struct CoroMeta {
    owner: Option<Entity>,
    access: CoroAccess,
}

#[derive(Default, Clone)]
pub struct CoroAccess {
    reads: HashMap<SourceId, SetUsize>,
    writes: HashMap<SourceId, SetUsize>,
}

#[derive(PartialEq, Eq, Clone, Copy, Hash)]
pub enum SourceId {
    Entity(Entity),
    AllEntities,
    World,
}

impl CoroAccess {
    /// Add a write access. Returns false if there is a conflict.
    /// The access is updated only when no conflicts are found.
    pub fn add_write(&mut self, to: SourceId, component: ComponentId) -> bool {
        if let Some(reads) = self.reads.get(&to) {
            if reads.contains(component.index()) {
                return false;
            }
        }

        self.writes.entry(to).or_default().insert(component.index())
    }

    /// Add a read access. Returns false if there is a conflict.
    /// The access is updated only when no conflicts are found.
    pub fn add_read(&mut self, to: SourceId, component: ComponentId) -> bool {
        if let Some(reads) = self.writes.get(&to) {
            if reads.contains(component.index()) {
                return false;
            }
        }

        self.reads.entry(to).or_default().insert(component.index());

        true
    }
}

pub struct CoroutineResult {
    result: CoroutineStatus,
    new_coro: Vec<NewCoroutine>,
    // TODO: triggered_signals?
}

/// A newly spawned [`Coroutine`] and how it should be handled by the [`Executor`](executor).
pub struct NewCoroutine {
    id: Id,
    coroutine: HeapCoro,
    is_owned_by_scope: bool,
    should_start_now: bool,
}

/// The status of the [`Coroutine`] after being resumed. If it is [`CoroutineStatus::Yield`], then
/// the coroutine should be resumed again once the condition is fullfiled. If it is [`Done`] then
/// the coroutine has finish execution and should not be resumed again. Doing so will panic.
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

#[cfg(test)]
mod test {
    use std::{
        sync::{Arc, Mutex},
        time::Instant,
    };

    use bevy::{ecs::system::Command, prelude::Mut, time::Time};

    use super::{commands::root_coroutine, executor::Executor, scope::Scope, *};

    #[test]
    fn wait_on_tick() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.insert_resource(Time::new(Instant::now()));

        let a = Arc::new(Mutex::new(0));
        let b = Arc::clone(&a);
        root_coroutine(|mut s: Scope| async move {
            *b.lock().unwrap() += 1;
            s.next_tick().await;
            *b.lock().unwrap() += 1;
            s.next_tick().await;
            *b.lock().unwrap() += 1;
        })
        .apply(&mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 1);
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 2);
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 3);
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 3);
        });
    }

    #[test]
    #[should_panic]
    fn await_external_future_panic() {
        async fn external_future() {
            std::future::pending::<()>().await;
        }
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.insert_resource(Time::new(Instant::now()));

        root_coroutine(|_: Scope| async move {
            external_future().await;
        })
        .apply(&mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            executor.tick(w);
        });
    }

    #[test]
    fn waiting_on_first() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.insert_resource(Time::new(Instant::now()));

        let a = Arc::new(Mutex::new(0));
        let b = Arc::clone(&a);

        root_coroutine(|mut fib: Scope| async move {
            let first = fib.start(|mut s: Scope| async move {
                loop {
                    s.next_tick().await;
                    *b.lock().unwrap() += 1;
                }
            });

            let second = fib.start(|mut s: Scope| async move {
                for _ in 0..4 {
                    s.next_tick().await;
                }
            });

            fib.first([first, second]).await;
        })
        .apply(&mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            for i in 0..5 {
                executor.tick(w);
                assert_eq!(*a.lock().unwrap(), i);
            }
        });
    }

    #[test]
    fn waiting_on_all() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.insert_resource(Time::new(Instant::now()));

        let a = Arc::new(Mutex::new(0));
        let b = Arc::clone(&a);
        let c = Arc::clone(&a);

        root_coroutine(|mut s: Scope| async move {
            let first = s.start(|mut s: Scope| async move {
                s.next_tick().await;
                *b.lock().unwrap() += 1;
            });
            let second = s.start(|mut s: Scope| async move {
                for _ in 0..2 {
                    s.next_tick().await;
                    *c.lock().unwrap() += 1;
                }
            });

            s.all((first, second)).await;
        })
        .apply(&mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 0);
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 2);
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 3);
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 3);
        });
    }

    #[test]
    fn waiting_on_first_result() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.insert_resource(Time::new(Instant::now()));

        root_coroutine(|mut fib: Scope| async move {
            let first = fib.start(|mut s: Scope| async move {
                loop {
                    s.next_tick().await;
                }
            });

            let second = fib.start(|mut s: Scope| async move {
                for _ in 0..2 {
                    s.next_tick().await;
                }
                10
            });

            let res = fib.first([first, second]).await;
            assert_eq!(res, 10);
        })
        .apply(&mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            executor.tick_until_empty(w);
        });
    }

    #[test]
    fn waiting_on_all_result() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.insert_resource(Time::new(Instant::now()));

        root_coroutine(|mut fib: Scope| async move {
            let first = fib.start(|mut s: Scope| async move {
                s.next_tick().await;
                20
            });

            let second = fib.start(|mut s: Scope| async move {
                for _ in 0..2 {
                    s.next_tick().await;
                }
                10
            });

            let res = fib.all((first, second)).await;
            assert_eq!(res, (20, 10));
        })
        .apply(&mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            executor.tick_until_empty(w);
        });
    }
}
