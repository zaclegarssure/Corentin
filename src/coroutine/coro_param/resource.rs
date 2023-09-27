use std::marker::PhantomData;

use bevy::{
    ecs::component::ComponentId,
    prelude::{self, Resource, World},
};

use crate::coroutine::{
    observable::{ObservableId, OnChange},
    SourceId,
};

use super::{CoroParam, ParamContext, RdGuard, WrGuard};

/// A readonly reference to a [`Resource`] in the [`World`].
///
/// Note that a Coroutine with such parameter will be canceled if the resource is removed.
pub struct RdRes<R: Resource> {
    context: ParamContext,
    id: ComponentId,
    _phantom: PhantomData<R>,
}

impl<R: Resource> CoroParam for RdRes<R> {
    fn init(
        context: ParamContext,
        world: &mut World,
        access: &mut crate::coroutine::CoroAccess,
    ) -> Option<Self> {
        let id = world.components().resource_id::<R>()?;
        if access.add_read(SourceId::World, id) {
            return None;
        }

        Some(RdRes {
            context,
            id,
            _phantom: PhantomData,
        })
    }

    fn is_valid(_owner: prelude::Entity, world: &World) -> bool {
        world.contains_resource::<R>()
    }
}

impl<R: Resource> RdRes<R> {
    /// Return the current value of the [`Resource`]. The result ([`RdGuard`]) cannot be held
    /// accros any await.
    pub fn get(&self) -> RdGuard<'_, R> {
        unsafe {
            RdGuard::new(
                self.context
                    .world_window
                    .world_cell()
                    .get_resource::<R>()
                    .unwrap(),
            )
        }
    }

    /// Yields and resume when the [`Resource`] is mutated.
    ///
    /// Note that it integrates with the regular change detection of Bevy, meaning that the
    /// coroutine will be resumed, if a [`System`] mutates the value.
    pub fn on_change(&self) -> OnChange<'_> {
        OnChange::new(&self.context, ObservableId::Resource(self.id))
    }
}

/// A read-write exclusive reference to a [`Resource`] in the [`World`].
///
/// Note that a Coroutine with such parameter will be canceled if the resource is removed.
pub struct WrRes<R: Resource> {
    context: ParamContext,
    id: ComponentId,
    _phantom: PhantomData<R>,
}

impl<R: Resource> CoroParam for WrRes<R> {
    fn init(
        context: ParamContext,
        world: &mut World,
        access: &mut crate::coroutine::CoroAccess,
    ) -> Option<Self> {
        let id = world.components().resource_id::<R>()?;
        if access.add_write(SourceId::World, id) {
            return None;
        }

        Some(WrRes {
            context,
            id,
            _phantom: PhantomData,
        })
    }

    fn is_valid(_owner: prelude::Entity, world: &World) -> bool {
        world.contains_resource::<R>()
    }
}

impl<R: Resource> WrRes<R> {
    /// Return the current value of the [`Resource`]. The result ([`RdGuard`]) cannot be held
    /// accros any await.
    pub fn get(&self) -> RdGuard<'_, R> {
        unsafe {
            RdGuard::new(
                self.context
                    .world_window
                    .world_cell()
                    .get_resource::<R>()
                    .unwrap(),
            )
        }
    }

    /// Return the current value of the [`Resource`]. The result ([`RdGuard`]) cannot be held
    /// accros any await.
    pub fn get_mut(&mut self) -> WrGuard<'_, R> {
        unsafe {
            WrGuard::new(
                self.context
                    .world_window
                    .world_cell()
                    .get_resource_mut::<R>()
                    .unwrap(),
            )
        }
    }

    /// Yields and resume when the [`Resource`] is mutated.
    ///
    /// Note that it integrates with the regular change detection of Bevy, meaning that the
    /// coroutine will be resumed, if a [`System`] mutates the value.
    pub fn on_change(&self) -> OnChange<'_> {
        OnChange::new(&self.context, ObservableId::Resource(self.id))
    }
}
