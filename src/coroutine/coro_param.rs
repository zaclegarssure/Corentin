use std::{
    cell::Cell,
    marker::PhantomData,
    ops::{Deref, DerefMut},
    rc::Rc,
};

use super::{
    observable::{ObservableId, OnChange},
    CoroWrites, WaitingReason,
};
use bevy::{
    ecs::{component::ComponentId, world::unsafe_world_cell::UnsafeWorldCell},
    prelude::{Component, Entity, Mut, World},
    utils::all_tuples,
};
use tinyset::SetUsize;

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

#[derive(Default)]
pub struct CoroAccess {
    this_reads: SetUsize,
    this_writes: SetUsize,
    //others: HashMap<>
}

/// A shared ref to a [`World`], it is "open" (meaning it points to a valid world with exclusive
/// access) when the Coroutine is resumed. It is shared to all [`CoroParam`]s.
#[derive(Clone)]
pub struct WorldWindow(Rc<Cell<*mut World>>);

// Safety: The window is only shared whith the coroutine itself, As with other similar construct,
// if you start spawning thread inside a coroutine, or sending coroutine parameters via channels,
// you may break the safety, therefore please don't do that. (thanks UwU)
unsafe impl Send for WorldWindow {}

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

/// All relevent values a [`CoroParam`] might need.
#[derive(Clone)]
pub struct ParamContext {
    pub(crate) owner: Entity,
    pub(crate) world_window: WorldWindow,
    pub(crate) yield_channel: YieldChannel,
}

/// Safety ? Who knows...
unsafe impl Send for ParamContext {}
unsafe impl Sync for ParamContext {}

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

        if access.this_writes.contains(id.index()) {
            return None;
        }

        access.this_reads.insert(id.index());

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

impl<T: Component> CoroParam for Wr<T> {
    fn init(context: ParamContext, w: &mut World, access: &mut CoroAccess) -> Option<Self> {
        let id = w.component_id::<T>().unwrap_or(w.init_component::<T>());

        if !access.this_reads.insert(id.index()) {
            return None;
        }

        access.this_writes.insert(id.index());

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
