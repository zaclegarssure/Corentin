use std::{
    ptr::{null, null_mut},
    time::Duration,
};

use bevy::{
    ecs::world::unsafe_world_cell::UnsafeWorldCell,
    prelude::{Entity, World},
    utils::synccell::SyncCell,
};

use crate::rework::{
    executor::{
        global_channel::GlobalSender,
        msg::{EmitMsg, NewCoroutine, SignalId},
    },
    id_alloc::{Id, Ids},
};

use super::{
    await_all::AwaitAll,
    await_first::AwaitFirst,
    await_time::{DurationFuture, NextTick},
    handle::{CoroHandle, HandleTuple},
    once_channel::{once_channel, Sender},
    resume::Resume,
    CoroStatus, CoroutineParamFunction, FunctionCoroutine, YieldMsg, await_signal::AwaitSignal,
};

/// The first parameter of any [`Coroutine`] It is used to spawn sub-coroutines, yield back to the
/// scheduler, queue commands and so on. It is the most unsafe part of this library, but once
/// proper coroutines are implemented in Rust, this would not be the case for the most part.
pub struct Scope {
    id: Id,
    owner: Option<Entity>,
    resume_param: *const ResumeParam,
    yield_sender: GlobalSender<YieldMsg>,
    new_coro_sender: GlobalSender<NewCoroutine>,
    emit_signal_sender: GlobalSender<EmitMsg>,
}

pub struct ResumeParam {
    pub(crate) world: *mut World,
    pub(crate) ids: *const Ids,
    pub(crate) is_paralel: bool,
    pub(crate) curr_node: usize,
}

impl ResumeParam {
    pub fn new() -> Self {
        Self {
            world: null_mut(),
            ids: null(),
            is_paralel: false,
            curr_node: 0,
        }
    }
}

impl Scope {
    pub(crate) fn new(
        id: Id,
        owner: Option<Entity>,
        resume_param: *const ResumeParam,
        yield_sender: GlobalSender<YieldMsg>,
        new_coro_sender: GlobalSender<NewCoroutine>,
        emit_signal_sender: GlobalSender<EmitMsg>,
    ) -> Self {
        Self {
            id,
            owner,
            resume_param,
            yield_sender,
            new_coro_sender,
            emit_signal_sender,
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
        let resume_param = Resume::new(ResumeParam::new());
        let new_scope = Self {
            id: self.alloc_id(),
            owner,
            resume_param: resume_param.get_raw(),
            yield_sender: self.yield_sender.clone(),
            new_coro_sender: self.new_coro_sender.clone(),
            emit_signal_sender: self.emit_signal_sender.clone(),
        };

        let new_id = new_scope.id;

        let coroutine = FunctionCoroutine::new(
            new_scope,
            self.world_cell_read_only(),
            resume_param,
            self.yield_sender.clone(),
            new_id,
            result_sender,
            coroutine,
        )?;

        self.add_new_coro(NewCoroutine {
            id: new_id,
            ran_after: self.curr_node(),
            coroutine: SyncCell::new(Box::pin(coroutine)),
            is_owned_by: parent_scope,
            should_start_now: start_now,
        });

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
        let (result_sender, receiver) = once_channel();
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

    /// Returns whenever we are currently running in paralel or not.
    pub fn is_paralel(&self) -> bool {
        unsafe { (*self.resume_param).is_paralel }
    }

    /// Returns the [`Entity`] owning this [`Coroutine`], if it exists.
    pub fn owner(&self) -> Option<Entity> {
        self.owner
    }

    /// Allocate a new unique coroutine id.
    pub fn alloc_id(&self) -> Id {
        unsafe { (*(*self.resume_param).ids).allocate_id() }
    }

    /// Send a [`YieldMsg`] with `status`, associated to this coroutine.
    pub fn yield_(&mut self, status: CoroStatus) {
        let msg = YieldMsg {
            id: self.id,
            node: self.curr_node(),
            status,
        };

        if self.is_paralel() {
            self.yield_sender.send(msg);
        } else {
            unsafe {
                self.yield_sender.send_sync(msg);
            }
        }
    }

    pub fn await_signal(&mut self, id: SignalId) -> AwaitSignal<'_> {

    }

    /// Returns the world as an [`UnsafeWorldCell`]. It should only be used.
    pub fn world_cell(&self) -> UnsafeWorldCell<'_> {
        unsafe { (*(*self.resume_param).world).as_unsafe_world_cell() }
    }

    pub fn world_cell_read_only(&self) -> UnsafeWorldCell<'_> {
        unsafe { (*(*self.resume_param).world).as_unsafe_world_cell_readonly() }
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
        unsafe { (*self.resume_param).curr_node }
    }
}

unsafe impl Send for Scope {}
