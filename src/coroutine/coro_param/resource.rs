use std::marker::PhantomData;

use bevy::{
    ecs::component::ComponentId,
    prelude::{self, Mut, Resource, World},
};

use crate::{
    coroutine::{
        observable::{ObservableId, OnChange},
        SourceId,
    },
    prelude::Fib,
};

use super::{CoroParam, ParamContext};

/// A readonly reference to a [`Resource`] in the [`World`].
///
/// Note that a Coroutine with such parameter will be canceled if the resource is removed.
pub struct RdRes<R: Resource> {
    id: ComponentId,
    _phantom: PhantomData<R>,
}

impl<R: Resource> CoroParam for RdRes<R> {
    fn init(
        _context: ParamContext,
        world: &mut World,
        access: &mut crate::coroutine::CoroAccess,
    ) -> Option<Self> {
        let id = world.components().resource_id::<R>()?;
        if access.add_read(SourceId::World, id) {
            return None;
        }

        Some(RdRes {
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
    pub fn get<'a>(&'a self, fib: &'a Fib) -> &'a R {
        unsafe { fib.world_window.world_cell().get_resource::<R>().unwrap() }
    }

    /// Yields and resume when the [`Resource`] is mutated.
    ///
    /// Note that it integrates with the regular change detection of Bevy, meaning that the
    /// coroutine will be resumed, if a [`System`] mutates the value.
    pub fn on_change<'a>(&'a self, fib: &'a mut Fib) -> OnChange<'a> {
        OnChange::new(fib, ObservableId::Resource(self.id))
    }
}

/// A read-write exclusive reference to a [`Resource`] in the [`World`].
///
/// Note that a Coroutine with such parameter will be canceled if the resource is removed.
pub struct WrRes<R: Resource> {
    id: ComponentId,
    _phantom: PhantomData<R>,
}

impl<R: Resource> CoroParam for WrRes<R> {
    fn init(
        _context: ParamContext,
        world: &mut World,
        access: &mut crate::coroutine::CoroAccess,
    ) -> Option<Self> {
        let id = world.components().resource_id::<R>()?;
        if access.add_write(SourceId::World, id) {
            return None;
        }

        Some(WrRes {
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
    pub fn get<'a>(&'a self, fib: &'a Fib) -> &'a R {
        unsafe { fib.world_window.world_cell().get_resource::<R>().unwrap() }
    }

    /// Return the current value of the [`Resource`]. The result ([`RdGuard`]) cannot be held
    /// accros any await.
    pub fn get_mut<'a>(&'a mut self, fib: &'a Fib) -> Mut<'a, R> {
        unsafe {
            fib.world_window
                .world_cell()
                .get_resource_mut::<R>()
                .unwrap()
        }
    }

    /// Yields and resume when the [`Resource`] is mutated.
    ///
    /// Note that it integrates with the regular change detection of Bevy, meaning that the
    /// coroutine will be resumed, if a [`System`] mutates the value.
    pub fn on_change<'a>(&'a self, fib: &'a mut Fib) -> OnChange<'a> {
        OnChange::new(fib, ObservableId::Resource(self.id))
    }
}
