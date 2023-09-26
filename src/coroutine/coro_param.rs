use std::{
    cell::Cell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use crate::executor::msg_channel::Sender;

use super::{
    observable::{ObservableId, OnChange},
    CoroMeta, CoroWrites, WaitingReason,
};
use bevy::{
    ecs::{component::ComponentId, world::unsafe_world_cell::UnsafeWorldCell},
    prelude::{Component, Entity, Mut, World},
    utils::all_tuples,
};

/// Any async function that takes only [`CoroParam`] as arguments can
/// automatically be turned into a [`Coroutine`].
pub trait CoroParam: Sized + Send + 'static {
    /// Initialize the parameter and register any access, if it is invalid (for instance
    /// conflicting accesses) it will return None.
    ///
    /// Note: `world_window` is not yet open at that point, `world` should be used instead.
    fn init(context: ParamContext, world: &mut World, meta: &mut CoroMeta) -> Option<Self>;
}

/// A shared ref to a [`World`], it is "open" (meaning it points to a valid world) when the
/// Coroutine is resumed.
#[derive(Clone)]
pub struct WorldWindow(pub(crate) Rc<Cell<*mut World>>);

// Safety: The window is only shared whith the coroutine itself
unsafe impl Send for WorldWindow {}

impl WorldWindow {
    /// # Safety
    /// The caller must ensure that the pointer points to a valid value. (the window should be
    /// "open").
    pub unsafe fn world_cell(&self) -> UnsafeWorldCell<'_> {
        (*self.0.get()).as_unsafe_world_cell()
    }

    /// Return the appropriate ComponentId, and initialize it if not present in the world
    ///
    /// # Safety
    /// The caller must ensure that the pointer points to a valid value. (the window should be
    /// "open"). And that it is called with exclusive world.
    pub unsafe fn component_id<T: Component>(&self) -> ComponentId {
        let cell = self.world_cell();
        cell.components()
            .component_id::<T>()
            .unwrap_or_else(|| cell.world_mut().init_component::<T>())
    }
}

/// All relevent values a [`CoroParam`] might need.
#[derive(Clone)]
pub struct ParamContext {
    pub(crate) owner: Entity,
    pub(crate) world_window: WorldWindow,
    pub(crate) yield_sender: Sender<WaitingReason>,
}

/// Safety ? Who knows...
unsafe impl Send for ParamContext {}
unsafe impl Sync for ParamContext {}

/// A readonly reference to a [`Component`] from the owning [`Entity`]
///
/// Note that a Coroutine with such parameter will be canceled if the entity does not have the
/// relevent component.
pub struct R<T: Component> {
    context: ParamContext,
    _phantom: PhantomData<T>,
}

unsafe impl<T: Component> Send for R<T> {}

impl<T: Component> CoroParam for R<T> {
    fn init(context: ParamContext, world: &mut World, meta: &mut CoroMeta) -> Option<Self> {
        let id = world
            .component_id::<T>()
            .unwrap_or(world.init_component::<T>());

        if meta.this_writes.contains(id.index()) {
            return None;
        }

        meta.this_reads.insert(id.index());

        Some(Self {
            context,
            _phantom: PhantomData,
        })
    }
}

/// A guarded readonly reference, cannot be hold accros awaiting points.
pub struct InGuard<'a, T> {
    value: &'a T,
    _phantom: PhantomData<*const T>,
}

impl<'a, T: Component> Deref for InGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T: Component> R<T> {
    pub fn get(&self) -> InGuard<'_, T> {
        unsafe {
            InGuard {
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

pub struct W<T: Component> {
    _phantom: PhantomData<T>,
    context: ParamContext,
}

pub struct InOutGuard<'a, T> {
    value: Mut<'a, T>,
    _phantom: PhantomData<*const T>,
}

impl<'a, T: Component> Deref for InOutGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.value.deref()
    }
}

impl<'a, T: Component> DerefMut for InOutGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.deref_mut()
    }
}

impl<T: Component> W<T> {
    pub fn get(&self) -> InGuard<'_, T> {
        let value = unsafe {
            self.context
                .world_window
                .world_cell()
                .get_entity(self.context.owner)
                .unwrap()
                .get::<T>()
                .unwrap()
        };

        InGuard {
            value,
            _phantom: PhantomData,
        }
    }

    pub fn get_mut(&mut self) -> InOutGuard<'_, T> {
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
            InOutGuard {
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

impl<T: Component> CoroParam for W<T> {
    fn init(context: ParamContext, w: &mut World, meta: &mut CoroMeta) -> Option<Self> {
        let id = w.component_id::<T>().unwrap_or(w.init_component::<T>());

        if !meta.this_reads.insert(id.index()) {
            return None;
        }

        meta.this_writes.insert(id.index());

        Some(Self {
            _phantom: PhantomData,
            context,
        })
    }
}

// TODO: Later
//impl<T: Component> CoroParam for Option<R<T>> {
//
//}

macro_rules! impl_coro_param {
    ($($param: ident),*) => {
        #[allow(non_snake_case, unused_parens, unused_variables)]
        impl<$($param: CoroParam),*> CoroParam for ($($param,)*) {
            fn init(context: ParamContext, world: &mut World, meta: &mut CoroMeta) -> Option<Self> {
                $(let $param = $param::init(context.clone(), world, meta)?;)*

                Some(($($param,)*))

            }
        }

    };
}

all_tuples!(impl_coro_param, 0, 16, P);
