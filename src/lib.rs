//! A coroutine library for the [bevy](https://github.com/bevyengine/bevy) game engine.
//!
//! TODO: Show example
pub mod commands;
pub mod coroutine;
pub mod executor;
pub mod plugin;

pub mod prelude {
    #[doc(hidden)]
    pub use crate::commands::{coroutine, AddCoroutine};
    #[doc(hidden)]
    pub use crate::coroutine::coro_param::{
        component::Rd, component::Wr, resource::RdRes, resource::WrRes, Opt,
    };
    #[doc(hidden)]
    pub use crate::coroutine::function_coroutine::Fib;
    #[doc(hidden)]
    pub use crate::executor::Executor;
    #[doc(hidden)]
    pub use crate::plugin::CorentinPlugin;
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Instant,
    };

    use bevy::{
        ecs::system::EntityCommand,
        prelude::{Component, Mut, World},
        time::Time,
    };

    use crate::{commands::coroutine, coroutine::CoroWrites, prelude::*};

    #[derive(Component)]
    struct ExampleComponent(u32);

    #[test]
    fn wait_on_tick() {
        let mut world = World::new();
        let e = world.spawn_empty().id();
        world.init_resource::<Executor>();
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));

        let a = Arc::new(Mutex::new(0));
        let b = Arc::clone(&a);
        coroutine(|mut fib: Fib| async move {
            *b.lock().unwrap() += 1;
            fib.next_tick().await;
            *b.lock().unwrap() += 1;
            fib.next_tick().await;
            *b.lock().unwrap() += 1;
        })
        .apply(e, &mut world);

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
    fn wait_on_external_change() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));

        let e = world.spawn(ExampleComponent(0)).id();
        let a = Arc::new(Mutex::new(0));
        let b = Arc::clone(&a);

        coroutine(|mut fib: Fib, ex: Rd<ExampleComponent>| async move {
            fib.next_tick().await;
            ex.on_change(&mut fib).await;
            *b.lock().unwrap() += 1;
        })
        .apply(e, &mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            w.clear_trackers();
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 0);
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 0);
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 0);
            w.entity_mut(e).get_mut::<ExampleComponent>().unwrap().0 += 1;
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 1);
            executor.tick(w);
            assert_eq!(*a.lock().unwrap(), 1);
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
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));

        coroutine(|_: Fib| async move {
            external_future().await;
        })
        .apply(world.spawn_empty().id(), &mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            executor.tick(w);
        });
    }

    #[test]
    fn waiting_on_par_or() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));

        let e = world.spawn_empty().id();
        let a = Arc::new(Mutex::new(0));
        let b = Arc::clone(&a);

        coroutine(|mut fib: Fib| async move {
            fib.par_or(|mut fib: Fib| async move {
                loop {
                    fib.next_tick().await;
                    *b.lock().unwrap() += 1;
                }
            })
            .with(|mut fib: Fib| async move {
                for _ in 0..4 {
                    fib.next_tick().await;
                }
            })
            .await;
        })
        .apply(e, &mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            for i in 0..5 {
                // Note that it works because the coroutine on the the top of the par_or,
                // has priority over the one on the bottom, meaning its side effect will be
                // seen on the last iteration. (Okay I just kind of gave)
                executor.tick(w);
                assert_eq!(*a.lock().unwrap(), i);
            }
        });
    }

    #[test]
    fn waiting_on_par_and() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));

        let e = world.spawn_empty().id();
        let a = Arc::new(Mutex::new(0));
        let b = Arc::clone(&a);
        let c = Arc::clone(&a);

        coroutine(|mut fib: Fib| async move {
            fib.par_and(|mut fib: Fib| async move {
                fib.next_tick().await;
                *b.lock().unwrap() += 1;
            })
            .with(|mut fib: Fib| async move {
                for _ in 0..2 {
                    fib.next_tick().await;
                    *c.lock().unwrap() += 1;
                }
            })
            .await;
        })
        .apply(e, &mut world);

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
    fn reading_components() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));

        let e = world.spawn(ExampleComponent(0)).id();

        coroutine(|mut fib: Fib, example: Rd<ExampleComponent>| async move {
            for i in 0..5 {
                fib.next_tick().await;
                assert_eq!(example.get(&fib).0, i);
            }
        })
        .apply(e, &mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            executor.tick(w);
            for _ in 0..5 {
                executor.tick(w);
                w.entity_mut(e).get_mut::<ExampleComponent>().unwrap().0 += 1;
            }
        });
    }

    #[test]
    fn writing_components() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));

        let e = world.spawn(ExampleComponent(0)).id();

        coroutine(
            |mut fib: Fib, mut example: Wr<ExampleComponent>| async move {
                for _ in 0..5 {
                    fib.next_tick().await;
                    example.get_mut(&fib).0 += 1;
                }
            },
        )
        .apply(e, &mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            for i in 0..5 {
                executor.tick(w);
                assert_eq!(w.entity_mut(e).get::<ExampleComponent>().unwrap().0, i)
            }
        });
    }

    #[test]
    fn waiting_on_internal_change() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));

        let e = world.spawn(ExampleComponent(0)).id();

        let a = Arc::new(Mutex::new(0));
        let b = Arc::clone(&a);

        coroutine(
            |mut fib: Fib, mut example: Wr<ExampleComponent>| async move {
                for _ in 0..5 {
                    fib.next_tick().await;
                    example.get_mut(&fib).0 += 1;
                }
            },
        )
        .apply(e, &mut world);

        coroutine(|mut fib: Fib, example: Rd<ExampleComponent>| async move {
            for _ in 0..5 {
                example.on_change(&mut fib).await;
                *b.lock().unwrap() += 1;
            }
        })
        .apply(e, &mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            for i in 0..5 {
                executor.tick(w);
                assert_eq!(*a.lock().unwrap(), i);
                w.clear_trackers();
            }
        });
    }

    #[test]
    fn waiting_on_internal_and_external_change_is_correct() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));
        let e = world.spawn(ExampleComponent(0)).id();

        let a = Arc::new(Mutex::new(0));
        let b = Arc::clone(&a);

        coroutine(|mut fib: Fib, ex: Rd<ExampleComponent>| async move {
            loop {
                ex.on_change(&mut fib).await;
                *b.lock().unwrap() += 1;
            }
        })
        .apply(e, &mut world);

        coroutine(|mut fib: Fib, mut ex: Wr<ExampleComponent>| async move {
            loop {
                fib.next_tick().await;
                ex.get_mut(&mut fib).0 += 1;
            }
        })
        .apply(e, &mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            for i in 0..5 {
                w.entity_mut(e).get_mut::<ExampleComponent>().unwrap().0 += 1;
                executor.tick(w);
                assert_eq!(*a.lock().unwrap(), i * 2);
                w.clear_trackers();
            }
        });
    }
}
