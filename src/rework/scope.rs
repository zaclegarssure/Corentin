use std::time::Duration;

use bevy::{ecs::world::unsafe_world_cell::UnsafeWorldCell, prelude::World};

use super::{
    all::AwaitAll,
    first::AwaitFirst,
    handle::{CoroHandle, HandleTuple},
    tick::{DurationFuture, NextTick},
    NewCoroutine, WaitingReason,
};

/// The first parameter of any [`Coroutine`] It is used to spawn sub-coroutines, yield back to the
/// scheduler, queue commands and so on. It is the most unsafe part of this library, but once
/// proper coroutines are implemented in Rust, this would not be the case for the most part.
pub struct Scope {
    world_ptr: *const *mut World,
    shared_yield: *mut Option<WaitingReason>,
    shared_new_coro: *mut Vec<NewCoroutine>,
}

impl Scope {
    /// Returns a future that resolve once all of the underlying coroutine finishes.
    pub fn all<H: HandleTuple>(&mut self, handles: H) -> AwaitAll<'_, H> {
        AwaitAll::new(self, handles)
    }

    /// Returns a future that resolve once any of the underlying coroutine finishes. Note that
    /// once this is done, all the others are dropped. The coroutines are resumed from top to
    /// bottom, in case multiple of them are ready to make progress at the same time.
    pub fn first<const N: usize, T>(&mut self, handles: [CoroHandle<T>; N]) -> AwaitFirst<'_, N, T>
    where
        T: Send + Sync + 'static,
    {
        AwaitFirst::new(self, handles)
    }

    /// Return a future that resolve once the underlying coroutine finishes.
    pub fn on<T>(&mut self, handle: CoroHandle<T>) -> AwaitFirst<'_, 1, T>
    where
        T: Send + Sync + 'static,
    {
        AwaitFirst::new(self, [handle])
    }

    /// Returns a future that resolve the next time the [`Executor`] is ticked (via
    /// [`run`][crate::executor::Executor::run] for instance). It returns the duration of the
    /// last frame (delta time).
    ///
    /// [`Executor`]: crate::executor::Executor
    pub fn next_tick(&mut self) -> NextTick<'_> {
        NextTick::new(self)
    }

    /// Returns a future that resolve after a certain [`Duration`]. Note that if the duration
    /// is smaller than the time between two tick of the [`Executor`] it won't be compensated.
    ///
    /// [`Executor`]: crate::executor::Executor
    pub fn duration(&mut self, duration: Duration) -> DurationFuture<'_> {
        DurationFuture::new(self, duration)
    }

    /// Check if the shared_zone is accessible, and panic otherwise
    fn check_shared(&self) {
        todo!()
    }

    pub fn set_waiting_reason(&mut self, reason: WaitingReason) {
        self.check_shared();
        unsafe {
            *self.shared_yield = Some(reason);
        }
    }

    pub fn world_cell(&self) -> UnsafeWorldCell<'_> {
        self.check_shared();
        unsafe { (**self.world_ptr).as_unsafe_world_cell() }
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum CoroState {
    Halted,
    Running,
}
