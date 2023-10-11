use bevy::ecs::system::{Command, EntityCommand};
use std::marker::PhantomData;

use bevy::prelude::{Entity, World};

use super::executor::Executor;
use super::function_coroutine::CoroutineParamFunction;

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
            executor.add_function_coroutine(Some(owner), world, self.coroutine);
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
            executor.add_function_coroutine(None, w, self.coroutine);
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
