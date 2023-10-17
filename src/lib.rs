use std::pin::Pin;

use bevy::ecs::component::ComponentId;
use bevy::prelude::Entity;
use bevy::prelude::World;
use bevy::utils::synccell::SyncCell;
use bevy::utils::HashMap;
use tinyset::SetUsize;

use self::executor::msg::CoroStatus;
use self::executor::msg::EmitMsg;
use self::executor::msg::NewCoroutine;

use self::id_alloc::Ids;

pub mod commands;
pub mod coro_param;
pub mod executor;
pub mod function_coroutine;
pub mod id_alloc;
pub mod plugin;

pub mod prelude {
    #[doc(hidden)]
    pub use crate::function_coroutine::prelude::*;

    #[doc(hidden)]
    pub use crate::coro_param::prelude::*;

    #[doc(hidden)]
    pub use crate::commands::*;

    #[doc(hidden)]
    pub use crate::plugin::*;
}

// THINGS MISSING:
// Dropping a scope should drop the local entities
// Commands
// SIGNALS !!!

/// A coroutine is a form of state machine. It can get resumed, and returns on which condition it
/// should be resumed again.
pub trait Coroutine: Send + 'static {
    /// Resume execution of this coroutine and returns it's new status.
    /// All other side effects are communicated back via channels.
    fn resume(
        self: Pin<&mut Self>,
        world: &mut World,
        ids: &Ids,
        curr_node: usize,
        next_coro_channel: &mut Vec<NewCoroutine>,
        emit_signal: &mut Vec<EmitMsg>,
    ) -> CoroStatus;

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

    use bevy::{
        ecs::system::{Command, EntityCommand},
        prelude::{Component, Mut},
        time::Time,
    };

    use super::{
        commands::{coroutine, root_coroutine},
        coro_param::{
            component::Wr,
            on_change::{ChangeTracker, OnChange},
        },
        executor::Executor,
        function_coroutine::scope::Scope,
        *,
    };

    #[derive(Component)]
    struct ExampleComponent(u32);

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

    #[test]
    fn waiting_on_internal_change() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.insert_resource(Time::new(Instant::now()));

        let e = world
            .spawn((
                ExampleComponent(0),
                ChangeTracker::new() as ChangeTracker<ExampleComponent>,
            ))
            .id();

        let a = Arc::new(Mutex::new(0));
        let b = Arc::clone(&a);

        coroutine(
            |mut fib: Scope, mut example: Wr<ExampleComponent>| async move {
                for _ in 0..5 {
                    fib.next_tick().await;
                    example.get_mut(&mut fib).0 += 1;
                }
            },
        )
        .apply(e, &mut world);

        coroutine(
            |mut fib: Scope, on_change: OnChange<ExampleComponent>| async move {
                for _ in 0..5 {
                    on_change.observe(&mut fib).await;
                    *b.lock().unwrap() += 1;
                }
            },
        )
        .apply(e, &mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            for i in 0..5 {
                executor.tick(w);
                assert_eq!(*a.lock().unwrap(), i);
            }
        });
    }
}
