use std::{
    ptr::{null, null_mut, NonNull},
    time::Duration,
};

use bevy::{
    ecs::{system::CommandQueue, world::unsafe_world_cell::UnsafeWorldCell},
    prelude::{Commands, Entity, World},
    utils::synccell::SyncCell,
};

use crate::{
    executor::msg::{EmitMsg, NewCoroutine, SignalId},
    id_alloc::{Id, Ids},
};

use super::{
    await_all::AwaitAll,
    await_first::AwaitFirst,
    await_time::{DurationFuture, NextTick},
    handle::{CoroHandle, HandleTuple},
    once_channel::{sync_once_channel, OnceSender},
    resume::Resume,
    CoroStatus, CoroutineParamFunction, FunctionCoroutine,
};

/// The first parameter of any [`Coroutine`] It is used to spawn sub-coroutines, yield back to the
/// scheduler, queue commands and so on. It is the most unsafe part of this library, but once
/// proper coroutines are implemented in Rust, this would not be the case for the most part.
pub struct Scope {
    id: Id,
    owner: Option<Entity>,
    resume_param: NonNull<ResumeParam>,
}

pub struct ResumeParam {
    pub(crate) world: *mut World,
    pub(crate) ids: *const Ids,
    pub(crate) curr_node: usize,
    pub(crate) yield_sender: Option<CoroStatus>,
    pub(crate) new_coro_sender: *mut Vec<NewCoroutine>,
    pub(crate) emit_sender: *mut Vec<EmitMsg>,
    pub(crate) commands: *mut CommandQueue,
}

impl Default for ResumeParam {
    fn default() -> Self {
        Self::new()
    }
}

impl ResumeParam {
    pub fn new() -> Self {
        Self {
            world: null_mut(),
            ids: null(),
            curr_node: 0,
            yield_sender: None,
            new_coro_sender: null_mut(),
            emit_sender: null_mut(),
            commands: null_mut(),
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn world_cell(&self) -> UnsafeWorldCell<'_> {
        unsafe { self.world.as_mut().unwrap().as_unsafe_world_cell() }
    }

    pub fn alloc_id(&self) -> Id {
        unsafe { self.ids.as_ref().unwrap().allocate_id() }
    }

    pub fn send_yield(&mut self, msg: CoroStatus) {
        self.yield_sender = Some(msg);
    }

    pub fn send_new_coro(&mut self, new_coro: NewCoroutine) {
        unsafe {
            self.new_coro_sender.as_mut().unwrap().push(new_coro);
        }
    }

    pub fn emit_signal(&mut self, id: SignalId) {
        unsafe {
            self.emit_sender.as_mut().unwrap().push(EmitMsg {
                id,
                by: self.curr_node,
            })
        }
    }

    pub fn curr_node(&self) -> usize {
        self.curr_node
    }
}

impl Scope {
    pub(crate) fn new(id: Id, owner: Option<Entity>, resume_param: NonNull<ResumeParam>) -> Self {
        Self {
            id,
            owner,
            resume_param,
        }
    }

    fn build_coroutine<Marker: 'static, T, C>(
        &mut self,
        owner: Option<Entity>,
        start_now: bool,
        parent_scope: Option<Id>,
        result_sender: Option<OnceSender<T>>,
        coroutine: C,
    ) -> Option<Id>
    where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        let self_params = self.resume_param_mut();
        let resume_param = Resume::new(ResumeParam::new());
        let new_scope = Self {
            id: self_params.alloc_id(),
            owner,
            resume_param: resume_param.get_raw(),
        };

        let new_id = new_scope.id;

        let coroutine = FunctionCoroutine::new(
            new_scope,
            self_params.world_cell(),
            resume_param,
            new_id,
            result_sender,
            coroutine,
        )?;

        self_params.send_new_coro(NewCoroutine {
            id: new_id,
            ran_after: self_params.curr_node(),
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
    pub fn start_local<Marker: 'static, T, C>(&mut self, coroutine: C)
    where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        self.build_coroutine(None, true, Some(self.id), None, coroutine);
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
        let (result_sender, receiver) = sync_once_channel();
        let id = self.build_coroutine(None, true, None, Some(result_sender), coroutine)?;
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

    pub fn deferred(&mut self) -> DeferredOps<'_> {
        DeferredOps::new(self)
    }

    pub fn commands(&mut self) -> Commands<'_, '_> {
        let queue = unsafe { self.resume_param.as_mut().commands.as_mut().unwrap() };
        let entities = unsafe { self.resume_param.as_mut().world_cell().entities() };
        Commands::new_from_entities(queue, entities)
    }

    /// Get a mutable reference to this scope [`ResumeParam`].
    ///
    /// # Safety
    /// This should be called only when the coroutine is polled,
    /// which should normally be the case if you have a mutable
    /// reference to the scope, but there might be ways to break this
    /// invariant.
    pub fn resume_param_mut(&mut self) -> &mut ResumeParam {
        unsafe { self.resume_param.as_mut() }
    }

    pub(crate) fn world_cell(&self) -> UnsafeWorldCell<'_> {
        unsafe { (*self.resume_param.as_ptr()).world_cell() }
    }

    pub(crate) fn emit_signal(&self, signal: SignalId) {
        unsafe { (*self.resume_param.as_ptr()).emit_signal(signal) }
    }

    pub fn yield_(&mut self, status: CoroStatus) {
        self.resume_param_mut().send_yield(status);
    }

    /// Returns the [`Entity`] owning this [`Coroutine`], if it exists.
    pub fn owner(&self) -> Option<Entity> {
        self.owner
    }
}

unsafe impl Send for Scope {}

pub struct DeferredOps<'a> {
    scope: &'a mut Scope,
}

impl<'a> DeferredOps<'a> {
    pub fn new(scope: &'a mut Scope) -> Self {
        Self { scope }
    }

    pub fn commands(&'a mut self, f: impl FnOnce(Commands<'a, 'a>)) {
        f(self.scope.commands())
    }

    pub fn spawn_local(&mut self) {
        todo!()
    }
}

pub struct DefferedLocal<'a> {
    id: Entity,
    scope: &'a mut Scope,
}

impl<'a> DefferedLocal<'a> {
    pub fn id(self) -> Entity {
        self.id
    }

    pub fn bind_coroutine<Marker: 'static, T, C>(self, coroutine: C) -> CoroHandle<T>
    where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        let (sender, receiver) = sync_once_channel();
        let id = self
            .scope
            .build_coroutine(
                Some(self.id),
                false,
                Some(self.scope.id),
                Some(sender),
                coroutine,
            )
            .unwrap();
        CoroHandle::Waiting { id, receiver }
    }
}
