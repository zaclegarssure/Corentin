use std::marker::PhantomData;

use bevy::{
    ecs::{component::ComponentId, world::unsafe_world_cell::UnsafeWorldCell},
    prelude::{Component, Entity, Mut},
};
use crate::{
    executor::msg::SignalId, function_coroutine::scope::Scope, id_alloc::Id, CoroMeta, SourceId,
};


use super::{on_change::ChangeTracker, CoroParam};

/// A readonly reference to a [`Component`] from the owning [`Entity`].
///
/// Note that a Coroutine with such parameter will be canceled if the entity does not have the
/// relevent component (or does not exist).
pub struct Rd<T: Component> {
    scope_id: Id,
    owner: Entity,
    _phantom: PhantomData<T>,
}

impl<T: Component> CoroParam for Rd<T> {
    fn init(world: UnsafeWorldCell<'_>, coro_meta: &mut CoroMeta) -> Option<Self> {
        let id = world.components().component_id::<T>()?;
        let owner = coro_meta.owner?;

        if !coro_meta.access.add_read(SourceId::Entity(owner), id) {
            return None;
        }

        Some(Self {
            scope_id: coro_meta.id,
            owner,
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

impl<T: Component> Rd<T> {
    /// Return the current value of the [`Component`]. The result ([`InGuard`]) cannot be held
    /// accros any await.
    pub fn get<'a>(&'a self, scope: &'a Scope) -> &'a T {
        scope.check_ownership(self.scope_id);
        unsafe {
            scope
                .world_cell()
                .get_entity(self.owner)
                .unwrap()
                .get::<T>()
                .unwrap()
        }
    }
}

/// A read-write exclusive reference to a [`Component`] from the owning [`Entity`].
///
/// Note that a Coroutine with such parameter will be canceled if the entity does not have the
/// relevent component.
pub struct Wr<T: Component> {
    owner: Entity,
    id: ComponentId,
    scope_id: Id,
    _phantom: PhantomData<T>,
}

impl<T: Component> CoroParam for Wr<T> {
    fn init(world: UnsafeWorldCell<'_>, coro_meta: &mut CoroMeta) -> Option<Self> {
        let id = world.components().component_id::<T>()?;
        let owner = coro_meta.owner?;

        if !coro_meta.access.add_write(SourceId::Entity(owner), id) {
            return None;
        }

        Some(Self {
            id,
            owner,
            scope_id: coro_meta.id,
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

impl<T: Component> Wr<T> {
    pub fn get<'a>(&'a mut self, scope: &'a Scope) -> &'a T {
        scope.check_ownership(self.scope_id);
        let value = unsafe {
            scope
                .world_cell()
                .get_entity(self.owner)
                .unwrap()
                .get::<T>()
                .unwrap()
        };

        value
    }

    pub fn get_mut<'a>(&'a mut self, scope: &'a Scope) -> Mut<'a, T> {
        scope.check_ownership(self.scope_id);

        unsafe {
            let cell = scope.world_cell();
            let entity = cell.get_entity(self.owner).unwrap();

            if entity.contains::<ChangeTracker<T>>() {
                scope.emit_signal(SignalId {
                    signal_type: self.id,
                    owner: Some(self.owner),
                });
            }

            cell.get_entity(self.owner).unwrap().get_mut::<T>().unwrap()
        }
    }
}
