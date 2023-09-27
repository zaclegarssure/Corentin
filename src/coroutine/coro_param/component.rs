use std::{
    marker::PhantomData,
    ops::{Deref, DerefMut},
};

use bevy::prelude::{Component, Entity, Mut, World};

use crate::coroutine::{
    observable::{ObservableId, OnChange},
    CoroAccess, CoroWrites, SourceId,
};

use super::{CoroParam, ParamContext};

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

/// A guarded readonly reference, cannot be hold accros awaiting points.
pub struct RdGuard<'a, T> {
    value: &'a T,
    _phantom: PhantomData<*const T>,
}

impl<'a, T: Component> Deref for RdGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T: Component> Rd<T> {
    /// Return the current value of the [`Component`]. The result ([`InGuard`]) cannot be held
    /// accros any await.
    pub fn get(&self) -> RdGuard<'_, T> {
        unsafe {
            RdGuard {
                value: self
                    .context
                    .world_window
                    .world_cell()
                    .get_entity(self.context.owner)
                    .unwrap()
                    .get::<T>()
                    .unwrap(),
                _phantom: PhantomData,
            }
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

pub struct WrGuard<'a, T> {
    value: Mut<'a, T>,
    _phantom: PhantomData<*const T>,
}

impl<'a, T: Component> Deref for WrGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.deref()
    }
}

impl<'a, T: Component> DerefMut for WrGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.deref_mut()
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

        RdGuard {
            value,
            _phantom: PhantomData,
        }
    }

    pub fn get_mut(&mut self) -> WrGuard<'_, T> {
        unsafe {
            let cell = self.context.world_window.world_cell();
            let c_id = cell.components().component_id::<T>().unwrap();
            cell.get_resource_mut::<CoroWrites>()
                .unwrap()
                .0
                .push_back((self.context.owner, c_id));

            let value = cell
                .get_entity(self.context.owner)
                .unwrap()
                .get_mut::<T>()
                .unwrap();
            WrGuard {
                value,
                _phantom: PhantomData,
            }
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
