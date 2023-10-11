use std::{
    ptr::{null, null_mut},
    time::Duration,
};

use bevy::{
    ecs::world::unsafe_world_cell::UnsafeWorldCell,
    prelude::{Entity, World},
};

use super::{
    all::AwaitAll,
    first::AwaitFirst,
    function_coroutine::{CoroutineParamFunction, FunctionCoroutine},
    global_channel::GlobalSender,
    handle::{CoroHandle, HandleTuple},
    id_alloc::{Id, Ids},
    one_shot::{channel, Sender},
    resume::Resume,
    tick::{DurationFuture, NextTick},
    CoroStatus, NewCoroutine, YieldMsg,
};

/// The first parameter of any [`Coroutine`] It is used to spawn sub-coroutines, yield back to the
/// scheduler, queue commands and so on. It is the most unsafe part of this library, but once
/// proper coroutines are implemented in Rust, this would not be the case for the most part.
pub struct Scope {
    id: Id,
    owner: Option<Entity>,
    world_ptr: *const *mut World,
    ids_ptr: *const *const Ids,
    is_paralel: *const bool,
    curr_node: *const usize,
    yield_sender: GlobalSender<YieldMsg>,
    new_coro_sender: GlobalSender<NewCoroutine>,
}

impl Scope {
    pub(crate) fn new(
        id: Id,
        owner: Option<Entity>,
        world_ptr: *const *mut World,
        ids_ptr: *const *const Ids,
        is_paralel: *const bool,
        curr_node: *const usize,
        yield_sender: GlobalSender<YieldMsg>,
        new_coro_sender: GlobalSender<NewCoroutine>,
    ) -> Self {
        Self {
            id,
            owner,
            world_ptr,
            ids_ptr,
            is_paralel,
            curr_node,
            yield_sender,
            new_coro_sender,
        }
    }

    fn build_coroutine<Marker: 'static, T, C>(
        &mut self,
        owner: Option<Entity>,
        start_now: bool,
        parent_scope: Option<Id>,
        result_sender: Option<Sender<T>>,
        coroutine: C,
    ) -> Option<Id>
    where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        let world_param = Resume::new(null_mut());
        let ids_param = Resume::new(null());
        let is_paralel = Resume::new(false);
        let curr_node = Resume::new(0);
        let new_scope = Self {
            id: self.alloc_id(),
            owner,
            world_ptr: world_param.get_raw(),
            ids_ptr: ids_param.get_raw(),
            is_paralel: is_paralel.get_raw(),
            curr_node: curr_node.get_raw(),
            yield_sender: self.yield_sender.clone(),
            new_coro_sender: self.new_coro_sender.clone(),
        };

        let new_id = new_scope.id;

        let coroutine = FunctionCoroutine::new(
            new_scope,
            self.world_cell_read_only(),
            world_param,
            ids_param,
            curr_node,
            is_paralel,
            self.yield_sender.clone(),
            new_id,
            result_sender,
            coroutine,
        )?;

        self.add_new_coro(NewCoroutine::new(
            new_id,
            self.curr_node(),
            coroutine,
            parent_scope,
            start_now,
        ));

        Some(new_id)
    }

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
    pub fn start_local<Marker: 'static, T, C>(&mut self, _coroutine: C)
    where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
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
        let (result_sender, receiver) = channel();
        let id = self.build_coroutine(None, true, Some(self.id), Some(result_sender), coroutine)?;
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
        self.build_coroutine(None, true, None, None, coroutine);
    }

    pub fn is_paralel(&self) -> bool {
        unsafe { *self.is_paralel }
    }

    pub fn owner(&self) -> Option<Entity> {
        self.owner
    }

    pub fn alloc_id(&self) -> Id {
        unsafe { (**self.ids_ptr).allocate_id() }
    }

    pub fn yield_(&mut self, msg: CoroStatus) {
        let msg = YieldMsg::new(self.id, self.curr_node(), msg);

        if self.is_paralel() {
            self.yield_sender.send(msg);
        } else {
            unsafe {
                self.yield_sender.send_sync(msg);
            }
        }
    }

    pub fn world_cell(&self) -> UnsafeWorldCell<'_> {
        unsafe { (**self.world_ptr).as_unsafe_world_cell() }
    }

    pub fn world_cell_read_only(&self) -> UnsafeWorldCell<'_> {
        unsafe { (**self.world_ptr).as_unsafe_world_cell_readonly() }
    }

    fn add_new_coro(&mut self, new_coro: NewCoroutine) {
        if self.is_paralel() {
            self.new_coro_sender.send(new_coro);
        } else {
            unsafe {
                self.new_coro_sender.send_sync(new_coro);
            }
        }
    }

    pub fn curr_node(&self) -> usize {
        unsafe { *self.curr_node }
    }
}

unsafe impl Send for Scope {}

#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum CoroState {
    Halted,
    Running,
}
