//! A coroutine library for the [bevy](https://github.com/bevyengine/bevy) game engine.
//!
//! TODO: Show example
pub mod coroutine;
pub mod executor;

mod waker;
mod world_window;

pub mod prelude {
    #[doc(hidden)]
    pub use crate::coroutine::Fib;
    #[doc(hidden)]
    pub use crate::executor::Executor;

    pub use crate::coroutine::duration::DurationFuture;
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Instant,
    };

    use bevy::{
        prelude::{Component, Mut, World},
        time::Time,
    };

    use crate::prelude::*;

    #[derive(Component)]
    struct ExampleComponent(u32);

    #[test]
    fn wait_on_tick() {
        let mut world = World::new();
        world.insert_resource(Executor::new());
        world.insert_resource(Time::new(Instant::now()));
        world.resource_scope(|w, mut executor: Mut<Executor>| {
            let a = Arc::new(Mutex::new(0));
            let b = Arc::clone(&a);
            executor.add(move |mut fib| async move {
                *b.lock().unwrap() += 1;
                fib.next_tick().await;
                *b.lock().unwrap() += 1;
                fib.next_tick().await;
                *b.lock().unwrap() += 1;
            });
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 1);
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 2);
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 3);
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 3);
        });
    }

    #[test]
    fn wait_on_sub_coroutine() {
        async fn sub_coro(mut fib: Fib) {
            fib.next_tick().await;
            fib.next_tick().await;
        }

        let mut world = World::new();
        world.insert_resource(Executor::new());
        world.insert_resource(Time::new(Instant::now()));
        world.resource_scope(|w, mut executor: Mut<Executor>| {
            let a = Arc::new(Mutex::new(0));
            let b = Arc::clone(&a);
            executor.add(move |mut fib| async move {
                *b.lock().unwrap() += 1;
                fib.on(sub_coro).await;
                *b.lock().unwrap() += 1;
            });
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 1);
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 1);
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 2);
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 2);
        });
    }

    #[test]
    fn wait_on_change() {
        let mut world = World::new();
        world.insert_resource(Executor::new());
        world.insert_resource(Time::new(Instant::now()));
        world.resource_scope(|w, mut executor: Mut<Executor>| {
            let e = w.spawn(ExampleComponent(0)).id();
            let a = Arc::new(Mutex::new(0));
            let b = Arc::clone(&a);
            executor.add(move |mut fib| async move {
                fib.next_tick().await;
                fib.change::<ExampleComponent>(e).await;
                *b.lock().unwrap() += 1;
            });
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 0);
            w.clear_trackers();
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 0);
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 0);
            w.entity_mut(e).get_mut::<ExampleComponent>().unwrap().0 += 1;
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 1);
            executor.run(w);
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
        world.insert_resource(Executor::new());
        world.insert_resource(Time::new(Instant::now()));
        world.resource_scope(|w, mut executor: Mut<Executor>| {
            executor.add(move |_| async move {
                external_future().await;
            });
            executor.run(w);
        });
    }

    #[test]
    fn waiting_on_par_or() {
        let mut world = World::new();
        world.insert_resource(Executor::new());
        world.insert_resource(Time::new(Instant::now()));
        world.resource_scope(|w, mut executor: Mut<Executor>| {
            let a = Arc::new(Mutex::new(0));
            let b = Arc::clone(&a);
            executor.add(move |mut fib| async move {
                fib.par_or(|mut fib| async move {
                    loop {
                        fib.next_tick().await;
                        *b.lock().unwrap() += 1;
                    }
                })
                .with(|mut fib| async move {
                    for _ in 0..4 {
                        fib.next_tick().await;
                    }
                })
                .await;
            });

            for i in 0..5 {
                // Note that it works because the coroutine on the the top of the par_or,
                // has priority over the one on the bottom, meaning its side effect will be
                // seen on the last iteration.
                executor.run(w);
                assert_eq!(*a.lock().unwrap(), i);
            }
        });
    }

    #[test]
    fn waiting_on_par_and() {
        let mut world = World::new();
        world.insert_resource(Executor::new());
        world.insert_resource(Time::new(Instant::now()));
        world.resource_scope(|w, mut executor: Mut<Executor>| {
            let a = Arc::new(Mutex::new(0));
            let b = Arc::clone(&a);
            let c = Arc::clone(&a);
            executor.add(move |mut fib| async move {
                fib.par_and(|mut fib| async move {
                    fib.next_tick().await;
                    *b.lock().unwrap() += 1;
                })
                .with(|mut fib| async move {
                    for _ in 0..2 {
                        fib.next_tick().await;
                        *c.lock().unwrap() += 1;
                    }
                })
                .await;
            });

            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 0);
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 2);
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 3);
            executor.run(w);
            assert_eq!(*a.lock().unwrap(), 3);
        });
    }
}
