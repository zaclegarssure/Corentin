use bevy::ecs::system::EntityCommand;
use bevy::utils::synccell::SyncCell;
use std::marker::PhantomData;

use bevy::prelude::{Entity, World};

use crate::coroutine::UninitCoroutine;
use crate::executor::Executor;

pub struct AddCoroutine<Marker, C> {
    coro: C,
    _phantom: PhantomData<Marker>,
}

impl<Marker: Send + 'static, C: Send + 'static> EntityCommand for AddCoroutine<Marker, C>
where
    C: UninitCoroutine<Marker>,
{
    fn apply(self, id: Entity, world: &mut World) {
        world.resource_scope::<Executor, ()>(|w, mut executor| {
            if let Some(coroutine) = self.coro.init(id, w) {
                executor.add(SyncCell::new(Box::pin(coroutine)));
            }
        });
    }
}

pub fn coroutine<M, C>(coro: C) -> AddCoroutine<M, C> {
    AddCoroutine::new(coro)
}

impl<M, C> AddCoroutine<M, C> {
    pub fn new(coro: C) -> Self {
        Self {
            coro,
            _phantom: PhantomData,
        }
    }
}
