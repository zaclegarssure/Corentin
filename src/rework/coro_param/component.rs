use std::marker::PhantomData;

use bevy::{
    ecs::{component::ComponentId, world::unsafe_world_cell::UnsafeWorldCell},
    prelude::{Component, Entity, Mut, World},
};

use crate::rework::{CoroMeta, SourceId, function_coroutine::scope::Scope};

use super::CoroParam;

///// A readonly reference to a [`Component`] from the owning [`Entity`].
/////
///// Note that a Coroutine with such parameter will be canceled if the entity does not have the
///// relevent component (or does not exist).
//pub struct Rd<T: Component> {
//    owner: Entity,
//    id: ComponentId,
//    _phantom: PhantomData<T>,
//}
//
//impl<T: Component> CoroParam for Rd<T> {
//    fn init(world: UnsafeWorldCell<'_>, coro_meta: &mut CoroMeta) -> Option<Self> {
//        let id = world.components().component_id::<T>()?;
//        let owner = coro_meta.owner?;
//
//        if !coro_meta.access.add_read(SourceId::Entity(owner), id) {
//            return None;
//        }
//
//        Some(Self {
//            owner,
//            id,
//            _phantom: PhantomData,
//        })
//    }
//}
//
//impl<T: Component> Rd<T> {
//    /// Return the current value of the [`Component`]. The result ([`InGuard`]) cannot be held
//    /// accros any await.
//    pub fn get<'a>(&'a self, scope: &'a Scope) -> &'a T {
//        unsafe {
//            scope
//                .world_cell()
//                .get_entity(self.owner)
//                .unwrap()
//                .get::<T>()
//                .unwrap()
//        }
//    }
//
//    /// Yields and resume when the `Component` is mutated.
//    ///
//    /// Note that it integrates with the regular change detection of Bevy, meaning that the
//    /// coroutine will be resumed, if a [`System`] mutates the value.
//    pub fn on_change<'a>(&'a self, fib: &'a mut Fib) -> OnChange<'a> {
//        OnChange::new(fib, ObservableId::Component(self.owner, self.id))
//    }
//}
//
///// A read-write exclusive reference to a [`Component`] from the owning [`Entity`].
/////
///// Note that a Coroutine with such parameter will be canceled if the entity does not have the
///// relevent component.
//pub struct Wr<T: Component> {
//    _phantom: PhantomData<T>,
//    owner: Entity,
//    id: ComponentId,
//}
//
//impl<T: Component> CoroParam for Wr<T> {
//    fn init(context: ParamContext, w: &mut World, access: &mut CoroAccess) -> Option<Self> {
//        let id = w.component_id::<T>().unwrap_or(w.init_component::<T>());
//
//        if !access.add_write(SourceId::Entity(context.owner), id) {
//            return None;
//        }
//
//        Some(Self {
//            _phantom: PhantomData,
//            id,
//            owner: context.owner,
//        })
//    }
//
//    fn is_valid(owner: Entity, world: &World) -> bool {
//        match world.get_entity(owner) {
//            Some(e) => e.contains::<T>(),
//            _ => false,
//        }
//    }
//}
//
//impl<T: Component> Wr<T> {
//    pub fn get<'a>(&'a self, fib: &'a Fib) -> &'a T {
//        let value = unsafe {
//            fib.world_window
//                .world_cell()
//                .get_entity(self.owner)
//                .unwrap()
//                .get::<T>()
//                .unwrap()
//        };
//
//        value
//    }
//
//    pub fn get_mut<'a>(&'a mut self, fib: &'a Fib) -> Mut<'a, T> {
//        unsafe {
//            let cell = fib.world_window.world_cell();
//            cell.get_resource_mut::<CoroWrites>()
//                .unwrap()
//                .0
//                // TODO fix write
//                .push_back((self.owner, self.id));
//
//            let value = cell.get_entity(self.owner).unwrap().get_mut::<T>().unwrap();
//
//            value
//        }
//    }
//
//    pub fn on_change<'a>(&'a self, fib: &'a mut Fib) -> OnChange<'a> {
//        OnChange::new(fib, ObservableId::Component(self.owner, self.id))
//    }
//}
