use std::{cell::Cell, rc::Rc};

use super::{CoroAccess, WaitingReason};
use bevy::{
    ecs::{component::ComponentId, world::unsafe_world_cell::UnsafeWorldCell},
    prelude::{Component, Entity, World},
    utils::all_tuples,
};

pub mod component;

/// Any async function that takes only [`CoroParam`] as arguments can
/// automatically be turned into a [`Coroutine`].
pub trait CoroParam: Sized + Send + 'static {
    /// Initialize the parameter and register any access, if it is invalid (for instance
    /// conflicting accesses) it will return None.
    ///
    /// Note: `world_window` is not yet open at that point, `world` should be used instead.
    fn init(context: ParamContext, world: &mut World, access: &mut CoroAccess) -> Option<Self>;

    /// Returns true if the parameter is still valid.
    fn is_valid(owner: Entity, world: &World) -> bool;
}

/// A shared ref to a [`World`], it is "open" (meaning it points to a valid world with exclusive
/// access) when the Coroutine is resumed. It is shared to all [`CoroParam`]s.
#[derive(Clone)]
pub struct WorldWindow(Rc<Cell<*mut World>>);

// Safety: The window is only shared whith the coroutine itself, As with other similar construct,
// if you start spawning thread inside a coroutine, or sending coroutine parameters via channels,
// you may break the safety, therefore please don't do that. (thanks UwU)
unsafe impl Send for WorldWindow {}
unsafe impl Sync for WorldWindow {}

impl WorldWindow {
    pub fn closed_window() -> Self {
        WorldWindow(Rc::new(Cell::new(std::ptr::null_mut())))
    }

    pub fn scope<T>(&mut self, world: &mut World, f: impl FnOnce() -> T) -> T {
        self.0.replace(world as *mut _);
        let res = f();
        self.0.replace(std::ptr::null_mut());
        res
    }

    /// Returns the world as an [`UnsafeWorldCell`].
    ///
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
    /// "open").
    pub unsafe fn component_id<T: Component>(&self) -> ComponentId {
        let cell = self.world_cell();
        cell.components()
            .component_id::<T>()
            .unwrap_or_else(|| cell.world_mut().init_component::<T>())
    }
}

/// The channel throught each [`CoroParam`] can yield back to the coroutine.
/// Which can then return the reason to the caller (the [`Executor`]).
#[derive(Default, Clone)]
pub struct YieldChannel(Rc<Cell<Option<WaitingReason>>>);

impl YieldChannel {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn receive(&self) -> Option<WaitingReason> {
        self.0.replace(None)
    }

    pub(crate) fn send(&self, val: WaitingReason) {
        self.0.replace(Some(val));
    }
}

// Safety: Same as [`WorldWindow`].
unsafe impl Send for YieldChannel {}
unsafe impl Sync for YieldChannel {}

/// All relevent values a [`CoroParam`] might need.
#[derive(Clone)]
pub struct ParamContext {
    pub(crate) owner: Entity,
    pub(crate) world_window: WorldWindow,
    pub(crate) yield_channel: YieldChannel,
}

pub struct Opt<T> {
    context: ParamContext,
    inner: T,
}

impl<T: CoroParam> Opt<T> {
    pub fn get(&self) -> Option<&T> {
        unsafe {
            if T::is_valid(
                self.context.owner,
                self.context.world_window.world_cell().world(),
            ) {
                Some(&self.inner)
            } else {
                None
            }
        }
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        unsafe {
            if T::is_valid(
                self.context.owner,
                self.context.world_window.world_cell().world(),
            ) {
                Some(&mut self.inner)
            } else {
                None
            }
        }
    }
}

impl<T: CoroParam> CoroParam for Opt<T> {
    fn init(context: ParamContext, world: &mut World, access: &mut CoroAccess) -> Option<Self> {
        let t = T::init(context.clone(), world, access)?;

        Some(Self { context, inner: t })
    }

    fn is_valid(_owner: Entity, _world: &World) -> bool {
        true
    }
}

macro_rules! impl_coro_param {
    ($($param: ident),*) => {
        #[allow(non_snake_case, unused_parens, unused_variables)]
        impl<$($param: CoroParam),*> CoroParam for ($($param,)*) {
            fn init(context: ParamContext, world: &mut World, access: &mut CoroAccess) -> Option<Self> {
                $(let $param = $param::init(context.clone(), world, access)?;)*

                Some(($($param,)*))

            }

            fn is_valid(owner: Entity, world: &World) -> bool {
                true $(&& $param::is_valid(owner, world))*
            }
        }

    };
}

all_tuples!(impl_coro_param, 0, 16, P);
