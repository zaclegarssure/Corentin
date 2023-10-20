use std::{ptr::NonNull, time::Duration};

use bevy::{
    ecs::world::unsafe_world_cell::UnsafeWorldCell,
    prelude::{Commands, Entity},
    utils::synccell::SyncCell,
};

use crate::{
    executor::msg::{EmitMsg, NewCoroutine, SignalId},
    id_alloc::Id,
};

use super::{
    await_all::AwaitAll,
    await_first::AwaitFirst,
    await_time::{DurationFuture, NextTick},
    handle::{CoroHandle, HandleTuple},
    once_channel::{sync_once_channel, OnceSender},
    resume::Resume,
    CoroStatus, CoroutineParamFunction, FunctionCoroutine, ResumeParam,
};

/// The first parameter of any [`Coroutine`] It is used to spawn sub-coroutines, yield back to the
/// scheduler, queue commands and so on. It is the most unsafe part of this library, but once
/// proper coroutines are implemented in Rust, this would not be the case for the most part.
pub struct Scope {
    id: Id,
    owner: Option<Entity>,
    resume_param: NonNull<ResumeParam>,
}

impl Scope {
    pub(crate) fn new(id: Id, owner: Option<Entity>, resume_param: NonNull<ResumeParam>) -> Self {
        Self {
            id,
            owner,
            resume_param,
        }
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
        self.build_coroutine(self.owner, true, Some(self.id), None, coroutine);
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
        let id = self.build_coroutine(self.owner, true, None, Some(result_sender), coroutine)?;
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

    /// Returns the [`Entity`] owning this [`Coroutine`], if it exists.
    pub fn owner(&self) -> Option<Entity> {
        self.owner
    }

    pub fn commands(&self) -> Commands<'_, '_> {
        unsafe {
            let entities = self.world_cell().entities();
            self.resume_param
                .as_ref()
                .commands_channel
                .as_ref()
                .unwrap()
                .commands(entities)
        }
    }

    pub fn bind_coroutine<Marker: 'static, T, C>(&self, to: Entity, coroutine: C) -> CoroHandle<T>
    where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        let (sender, receiver) = sync_once_channel();
        let id = self
            .build_coroutine(Some(to), false, Some(self.id), Some(sender), coroutine)
            .unwrap();
        CoroHandle::Waiting { id, receiver }
    }

    pub(crate) fn world_cell(&self) -> UnsafeWorldCell<'_> {
        unsafe {
            self.resume_param
                .as_ref()
                .world
                .as_mut()
                .unwrap()
                .as_unsafe_world_cell()
        }
    }

    /// Emit the given signal
    pub(crate) fn emit_signal(&self, id: SignalId) {
        // Safety: None, fuck it
        unsafe {
            let by = self.resume_param.as_ref().curr_node;
            let sender = self.resume_param.as_ref().emit_channel.as_ref().unwrap();
            sender.send(EmitMsg { id, by });
        };
    }

    /// Yield with the following status
    pub(crate) fn yield_(&mut self, status: CoroStatus) {
        // Safety: When polled, the scope owns CoroParam which own each parameter
        unsafe {
            self.resume_param.as_mut().yield_sender = Some(status);
        }
    }

    /// Send a new coroutine to the executor
    fn send_new_coro(&self, new_coro: NewCoroutine) {
        unsafe {
            self.resume_param
                .as_ref()
                .new_coro_channel
                .as_ref()
                .unwrap()
                .send(new_coro);
        }
    }

    /// Allocate a new unique coroutine id
    fn alloc_id(&self) -> Id {
        unsafe {
            self.resume_param
                .as_ref()
                .ids
                .as_ref()
                .unwrap()
                .allocate_id()
        }
    }

    /// Build a new coroutine with various parameter
    fn build_coroutine<Marker: 'static, T, C>(
        &self,
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
        let resume_param = Resume::new(ResumeParam::new());
        let new_scope = Self {
            id: self.alloc_id(),
            owner,
            resume_param: resume_param.get_raw(),
        };

        let new_id = new_scope.id;

        let coroutine = FunctionCoroutine::new(
            new_scope,
            self.world_cell(),
            resume_param,
            new_id,
            result_sender,
            coroutine,
        )?;

        self.send_new_coro(NewCoroutine {
            id: new_id,
            ran_after: self.curr_node(),
            coroutine: SyncCell::new(Box::pin(coroutine)),
            is_owned_by: parent_scope,
            should_start_now: start_now,
        });

        Some(new_id)
    }

    fn curr_node(&self) -> usize {
        unsafe { self.resume_param.as_ref().curr_node }
    }
}

unsafe impl Send for Scope {}

//pub struct DeferredOps<'a> {
//    scope: &'a Scope,
//    queue: CommandQueue,
//}
//
//impl<'a> DeferredOps<'a> {
//    pub fn new(scope: &'a mut Scope) -> Self {
//        Self {
//            scope,
//            queue: CommandQueue::default(),
//        }
//    }
//
//    pub fn apply(self) {
//        self.scope.command_sender.send(self.queue).unwrap();
//    }
//
//    pub fn commands(&mut self, f: impl FnOnce(Commands<'_, '_>)) -> &mut Self {
//        let entities = self.scope.world_cell().entities();
//        let commands = Commands::new_from_entities(&mut self.queue, entities);
//        f(commands);
//        self
//    }
//
//
//    //pub fn spawn_local(&mut self) -> DefferedLocal<'_> {
//    //    todo!()
//    //}
//}

//pub struct DefferedLocal<'a> {
//    id: Entity,
//    scope: &'a Scope,
//}
//
//impl<'a> DefferedLocal<'a> {
//    pub fn id(self) -> Entity {
//        self.id
//    }
//
//    pub fn bind_coroutine<Marker: 'static, T, C>(self, coroutine: C) -> CoroHandle<T>
//    where
//        C: CoroutineParamFunction<Marker, T>,
//        T: Sync + Send + 'static,
//    {
//        let (sender, receiver) = sync_once_channel();
//        let id = self
//            .scope
//            .build_coroutine(
//                Some(self.id),
//                false,
//                Some(self.scope.id),
//                Some(sender),
//                coroutine,
//            )
//            .unwrap();
//        CoroHandle::Waiting { id, receiver }
//    }
//
//    pub fn add(&mut self, command: impl EntityCommand) -> &mut Self {
//        self.scope.commands().entity(self.id).add(command);
//        self
//    }
//
//    pub fn insert(&mut self, bundle: impl Bundle) -> &mut Self {
//        self.scope.commands().add(Insert {
//            entity: self.id,
//            bundle,
//        });
//        self
//    }
//}
