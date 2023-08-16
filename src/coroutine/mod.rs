use std::cell::Cell;
use std::future::Future;
use std::rc::Rc;
use std::time::Duration;

use bevy::ecs::component::ComponentId;
use bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell;
use bevy::prelude::Component;
use bevy::prelude::Entity;
use bevy::prelude::Timer;
use bevy::prelude::World;

use crate::executor::msg_channel::Sender;
use crate::executor::CoroObject;

use duration::{DurationFuture, NextTick};
use on::On;
use par_and::ParAnd;
use par_or::ParOr;
use when::Change;

use self::grab::GrabReason;

pub mod duration;
pub mod grab;
mod on;
mod par_and;
mod par_or;
mod when;

#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum CoroState {
    Halted,
    Running,
}

pub(crate) enum WaitingReason {
    Tick,
    Duration(Timer),
    Changed {
        from: Entity,
        component: ComponentId,
    },
    ParOr {
        coroutines: Vec<CoroObject>,
    },
    ParAnd {
        coroutines: Vec<CoroObject>,
    },
}

/// A "Fiber" object, througth which a coroutine
/// can interact with the rest of the world.
#[derive(Clone)]
pub struct Fib {
    // Maybe replace by a real sender receiver channel at some point
    pub(crate) yield_sender: Sender<WaitingReason>,
    pub(crate) owner: Option<Entity>,
    pub(crate) world_window: Rc<Cell<Option<*mut World>>>,
    pub(crate) grab_sender: Sender<GrabReason>,
}

impl Fib {
    fn component_id<T: Component>(&self) -> ComponentId {
        // SAFETY: We are in the polling phase, therefore the coroutine is the only one running.
        unsafe {
            let w = &mut *self
                .world_window
                .get()
                .expect("This function should have been called when a coroutine is polled");
            w.component_id::<T>()
                .expect("Component should have been initialized in the world")
        }
    }

    pub(crate) unsafe fn world_window(&self) -> UnsafeWorldCell<'_> {
        let a = &mut *self
            .world_window
            .get()
            .expect("This function should have been called when a coroutine is polled");
        a.as_unsafe_world_cell()
    }
}

// SAFETY: Same as Executor, the sender field is only accessed when polled,
// which is done in a single threaded context.
unsafe impl Send for Fib {}
unsafe impl Sync for Fib {}

impl Fib {
    /// Returns coroutine that resolve the next time the [`Executor`] is ticked (via
    /// [`run`][crate::executor::Executor::run] for instance). It returns the duration
    /// of the last frame (delta time).
    ///
    /// [`Executor`]: crate::executor::Executor
    pub fn next_tick(&mut self) -> NextTick<'_> {
        NextTick::new(self.clone())
    }

    /// Returns a coroutine that resolve after a certain [`Duration`]. Note that if the duration
    /// is smaller than the time between two tick of the [`Executor`] it won't be compensated.
    ///
    /// [`Executor`]: crate::executor::Executor
    pub fn duration(&mut self, duration: Duration) -> DurationFuture<'_> {
        DurationFuture::new(self.clone(), duration)
    }

    /// Returns a coroutine that resolve once the [`Component`] of type `T` from a particular
    /// [`Entity`] has changed. Note that if the entity or the components is removed,
    /// this coroutine is dropped.
    pub fn change<T: Component + Unpin>(&mut self, from: Entity) -> Change<'_, T> {
        Change::new(self.clone(), from)
    }

    /// Returns a coroutine that resolve once any of the underlying coroutine finishes. Note that
    /// once this is done, all the others are dropped. The coroutines are resumed from top to
    /// bottom, in case multiple of them are ready to make progress at the same time.
    pub fn par_or<C, F>(&mut self, closure: C) -> ParOr<'_>
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib) -> F,
    {
        let fib = self.clone();
        let fut = Box::pin(closure(fib));
        ParOr::new(self.clone(), vec![fut])
    }

    /// Returns a coroutine that resolve once the underlying coroutine finishes,
    /// in order to reuse coroutines, because the following won't compile:
    /// ```compile_fail
    ///# use corentin::prelude::*;
    ///async fn sub_coro(mut fib: Fib) { }
    ///async fn main_coro(mut fib: Fib) {
    ///  sub_coro(fib).await;
    ///  sub_coro(fib).await;
    ///}
    ///```
    /// But the following will:
    ///```
    ///# use corentin::prelude::*;
    ///async fn sub_coro(mut fib: Fib) { }
    ///async fn main_coro(mut fib: Fib) {
    ///  fib.on(sub_coro).await;
    ///  fib.on(sub_coro).await;
    ///}
    ///```
    pub fn on<C, F>(&mut self, closure: C) -> On<F>
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib) -> F,
    {
        let fib = self.clone();
        let fut = closure(fib);
        On::new(fut)
    }

    /// Same as [`Self::on()`] but with a coroutine that expect
    /// an owner entity as a parameter.
    pub fn on_self<C, F>(&mut self, closure: C) -> On<F>
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib, Entity) -> F,
    {
        let fib = self.clone();
        let fut = closure(
            fib,
            self.owner
                .expect("Cannot call on_self if the coroutine is not owned by any entity."),
        );
        On::new(fut)
    }

    /// Returns a coroutine that resolve once all of the underlying coroutine finishes. The
    /// coroutines are resumed from top to bottom, in case multiple of them are ready to make
    /// progress at the same time.
    pub fn par_and<C, F>(&mut self, closure: C) -> ParAnd<'_>
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib) -> F,
    {
        let fib = self.clone();
        let fut = Box::pin(closure(fib));
        ParAnd::new(self.clone(), vec![fut])
    }

    pub fn owner(&self) -> Option<Entity> {
        self.owner
    }
}
