use std::time::Duration;

use bevy::{
    ecs::world::unsafe_world_cell::UnsafeWorldCell,
    prelude::{Entity, World},
    utils::synccell::SyncCell,
};

use super::{
    all::AwaitAll,
    first::AwaitFirst,
    function_coroutine::{CoroutineParamFunction, FunctionCoroutine},
    handle::{CoroHandle, HandleTuple},
    id_alloc::{Id, Ids},
    tick::{DurationFuture, NextTick},
    Coroutine, NewCoroutine, WaitingReason,
};

/// The first parameter of any [`Coroutine`] It is used to spawn sub-coroutines, yield back to the
/// scheduler, queue commands and so on. It is the most unsafe part of this library, but once
/// proper coroutines are implemented in Rust, this would not be the case for the most part.
pub struct Scope {
    pub(crate) owner: Entity,
    pub(crate) world_ptr: *const *mut World,
    pub(crate) ids_ptr: *const *const Ids,
    pub(crate) shared_yield: *mut Option<WaitingReason>,
    pub(crate) shared_new_coro: *mut Vec<NewCoroutine>,
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

    /// Start the `coroutine` when reaching the next `await`. When the scope is dropped, the
    /// `coroutine` is automatically dropped as well.
    ///
    /// Note: If the coroutine is invalid (with conflicting parameters for instance), this function
    /// has no effects.
    pub fn start_local<Marker: 'static, T, C>(&mut self, coroutine: C)
    where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        if let Some(coroutine) = FunctionCoroutine::from_function(self, self.owner, None, coroutine)
        {
            let id = self.alloc_id();
            self.add_new_coro(coroutine, id, true, true);
        }
    }

    /// Start the `coroutine` when reaching the next `await`, and returns a [`CoroHandle`] to it.
    /// When the handle is dropped, the `coroutine` is automatically dropped as well.
    ///
    /// Note: If the coroutine is invalid (with conflicting parameters for instance), this function
    /// panics.
    pub fn start<Marker: 'static, T, C>(&mut self, coroutine: C) -> CoroHandle<T>
    where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        self.try_start(coroutine).unwrap()
    }

    /// Start the `coroutine` when reaching the next `await`, and returns a [`CoroHandle`] to it.
    /// When the handle is dropped, the `coroutine` is automatically dropped as well.
    /// If the coroutine is invalid (with conflicting parameters for instance), this function
    /// returns None.
    pub fn try_start<Marker: 'static, T, C>(&mut self, coroutine: C) -> Option<CoroHandle<T>>
    where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        let (sender, receiver) = oneshot::channel();
        let coroutine =
            FunctionCoroutine::from_function(self, self.owner, Some(sender), coroutine)?;
        let id = self.alloc_id();
        self.add_new_coro(coroutine, id, false, true);
        Some(CoroHandle::Waiting { id, receiver })
    }

    /// Start the `coroutine` when reaching the next `await`. The coroutine cannot be dropped, and
    /// will be run until completion. This is unstructured and must be used with caution.
    ///
    /// Note: If the coroutine is invalid (with conflicting parameters for instance), this function
    /// has no effects.
    pub fn start_forget<Marker: 'static, T, C>(&mut self, coroutine: C)
    where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        if let Some(coroutine) = FunctionCoroutine::from_function(self, self.owner, None, coroutine)
        {
            let id = self.alloc_id();
            self.add_new_coro(coroutine, id, false, true);
        }
    }

    pub fn owner(&self) -> Entity {
        self.owner()
    }

    pub fn alloc_id(&self) -> Id {
        unsafe { (**self.ids_ptr).allocate_id() }
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

    fn add_new_coro(
        &mut self,
        coro: impl Coroutine,
        id: Id,
        is_owned_by_scope: bool,
        should_start_now: bool,
    ) {
        unsafe {
            (*self.shared_new_coro).push(NewCoroutine {
                id,
                coroutine: SyncCell::new(Box::pin(coro)),
                is_owned_by_scope,
                should_start_now,
            })
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum CoroState {
    Halted,
    Running,
}
