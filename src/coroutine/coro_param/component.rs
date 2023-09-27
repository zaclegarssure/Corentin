use std::marker::PhantomData;

use bevy::prelude::{Component, Entity, World};

use crate::coroutine::{
    observable::{ObservableId, OnChange},
    CoroAccess, CoroWrites, SourceId,
};

use super::{CoroParam, ParamContext, RdGuard, WrGuard};

/// A readonly reference to a [`Component`] from the owning [`Entity`].
///
/// Note that a Coroutine with such parameter will be canceled if the entity does not have the
/// relevent component.
pub struct Rd<T: Component> {
    context: ParamContext,
    _phantom: PhantomData<T>,
}

impl<T: Component> CoroParam for Rd<T> {
    fn init(context: ParamContext, world: &mut World, access: &mut CoroAccess) -> Option<Self> {
        let id = world
            .component_id::<T>()
            .unwrap_or(world.init_component::<T>());

        if !access.add_read(SourceId::Entity(context.owner), id) {
            return None;
        }

        Some(Self {
            context,
            _phantom: PhantomData,
        })
    }

    fn is_valid(owner: Entity, world: &World) -> bool {
        match world.get_entity(owner) {
            Some(e) => e.contains::<T>(),
            _ => false,
        }
    }
}

impl<T: Component> Rd<T> {
    /// Return the current value of the [`Component`]. The result ([`InGuard`]) cannot be held
    /// accros any await.
    pub fn get(&self) -> RdGuard<'_, T> {
        unsafe {
            RdGuard::new(
                self.context
                    .world_window
                    .world_cell()
                    .get_entity(self.context.owner)
                    .unwrap()
                    .get::<T>()
                    .unwrap(),
            )
        }
    }

    /// Yields and resume when the `Component` is mutated.
    ///
    /// Note that it integrates with the regular change detection of Bevy, meaning that the
    /// coroutine will be resumed, if a [`System`] mutates the value.
    pub fn on_change(&self) -> OnChange<'_> {
        unsafe {
            OnChange::new(
                &self.context,
                ObservableId::Component(
                    self.context.owner,
                    self.context.world_window.component_id::<T>(),
                ),
            )
        }
    }
}

/// A read-write exclusive reference to a [`Component`] from the owning [`Entity`].
///
/// Note that a Coroutine with such parameter will be canceled if the entity does not have the
/// relevent component.
pub struct Wr<T: Component> {
    _phantom: PhantomData<T>,
    context: ParamContext,
}

impl<T: Component> CoroParam for Wr<T> {
    fn init(context: ParamContext, w: &mut World, access: &mut CoroAccess) -> Option<Self> {
        let id = w.component_id::<T>().unwrap_or(w.init_component::<T>());

        if !access.add_write(SourceId::Entity(context.owner), id) {
            return None;
        }

        Some(Self {
            _phantom: PhantomData,
            context,
        })
    }

    fn is_valid(owner: Entity, world: &World) -> bool {
        match world.get_entity(owner) {
            Some(e) => e.contains::<T>(),
            _ => false,
        }
    }
}

impl<T: Component> Wr<T> {
    pub fn get(&self) -> RdGuard<'_, T> {
        let value = unsafe {
            self.context
                .world_window
                .world_cell()
                .get_entity(self.context.owner)
                .unwrap()
                .get::<T>()
                .unwrap()
        };

        RdGuard::new(value)
    }

    pub fn get_mut(&mut self) -> WrGuard<'_, T> {
        unsafe {
            let cell = self.context.world_window.world_cell();
            let c_id = cell.components().component_id::<T>().unwrap();
            cell.get_resource_mut::<CoroWrites>()
                .unwrap()
                .0
                // TODO fix write
                .push_back((self.context.owner, c_id));

            let value = cell
                .get_entity(self.context.owner)
                .unwrap()
                .get_mut::<T>()
                .unwrap();

            WrGuard::new(value)
        }
    }

    pub fn on_change(&self) -> OnChange<'_> {
        unsafe {
            OnChange::new(
                &self.context,
                ObservableId::Component(
                    self.context.owner,
                    self.context.world_window.component_id::<T>(),
                ),
            )
        }
    }
}
