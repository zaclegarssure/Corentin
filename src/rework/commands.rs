use bevy::ecs::system::{Command, EntityCommand};
use bevy::utils::synccell::SyncCell;
use std::marker::PhantomData;

use bevy::prelude::{Entity, World};

use super::executor::Executor;
use super::function_coroutine::{CoroutineParamFunction, FunctionCoroutine};

pub struct AddRootCoroutine<Marker, T, C> {
    coroutine: C,
    _phantom1: PhantomData<Marker>,
    _phantom2: PhantomData<T>,
}

pub struct AddCoroutineTo<Marker, T, C> {
    coroutine: C,
    _phantom1: PhantomData<Marker>,
    _phantom2: PhantomData<T>,
}

impl<Marker, C, T> EntityCommand for AddCoroutineTo<Marker, T, C>
where
    C: CoroutineParamFunction<Marker, T>,
    T: Sync + Send + 'static,
    Marker: 'static + Send,
{
    fn apply(self, owner: Entity, world: &mut World) {
        world.resource_scope::<Executor, ()>(|world, mut executor| {
            if let Some(coroutine) =
                FunctionCoroutine::from_world(world, Some(owner), None, self.coroutine)
            {
                executor.add_coroutine(SyncCell::new(Box::pin(coroutine)));
            }
        });
    }
}

impl<Marker, C, T> Command for AddRootCoroutine<Marker, T, C>
where
    C: CoroutineParamFunction<Marker, T>,
    T: Sync + Send + 'static,
    Marker: 'static + Send,
{
    fn apply(self, world: &mut World) {
        world.resource_scope::<Executor, ()>(|w, mut executor| {
            if let Some(coroutine) = FunctionCoroutine::from_world(w, None, None, self.coroutine) {
                executor.add_coroutine(SyncCell::new(Box::pin(coroutine)));
            }
        });
    }
}

pub fn root_coroutine<M, C, T>(coroutine: C) -> AddRootCoroutine<M, T, C> {
    AddRootCoroutine {
        coroutine,
        _phantom1: PhantomData,
        _phantom2: PhantomData,
    }
}

pub fn coroutine<M, C, T>(coroutine: C) -> AddCoroutineTo<M, T, C> {
    AddCoroutineTo {
        coroutine,
        _phantom1: PhantomData,
        _phantom2: PhantomData,
    }
}
