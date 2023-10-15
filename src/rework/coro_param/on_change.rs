use std::marker::PhantomData;

use bevy::{ecs::world::unsafe_world_cell::UnsafeWorldCell, prelude::Component};

use crate::rework::{
    executor::msg::SignalId,
    function_coroutine::{await_change::AwaitChange, scope::Scope},
    CoroMeta,
};

use super::CoroParam;

#[derive(Component)]
pub struct ChangeTracker<T: Component> {
    _phantom: PhantomData<T>,
}

impl<T: Component> ChangeTracker<T> {
    pub fn new() -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

pub struct OnChange<T: Component> {
    id: SignalId,
    _phantom: PhantomData<T>,
}

impl<T: Component> OnChange<T> {
    pub fn observe<'a>(&self, scope: &'a mut Scope) -> AwaitChange<'a> {
        AwaitChange::new(scope, self.id)
    }
}

impl<T: Component> CoroParam for OnChange<T> {
    fn init(world: UnsafeWorldCell<'_>, coro_meta: &mut CoroMeta) -> Option<Self> {
        let id = world.components().component_id::<T>()?;
        let owner = coro_meta.owner?;

        Some(Self {
            id: SignalId {
                signal_type: id,
                owner: Some(owner),
            },
            _phantom: PhantomData,
        })
    }

    fn is_valid(world: UnsafeWorldCell<'_>, coro_meta: &CoroMeta) -> bool {
        if let Some(owner) = coro_meta.owner {
            if let Some(entity) = world.get_entity(owner) {
                return entity.contains::<T>();
            }
        }

        false
    }
}
