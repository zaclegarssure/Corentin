use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bevy::{
    ecs::{component::ComponentId, world::unsafe_world_cell::UnsafeEntityCell},
    prelude::{Component, Entity, Mut, Ref, World},
    utils::all_tuples,
};
use pin_project::pin_project;
use tinyset::SetUsize;

use super::Fib;

/// Describe what world access will be perform by a coroutine once resumed
/// TODO: Benchmark and probably use something like tiny set or whatnot
pub(crate) enum GrabReason {
    Single(GrabAccess),
    Multi(Vec<GrabAccess>),
}

impl GrabReason {
    pub fn writes(&self) -> Vec<(Entity, ComponentId)> {
        match self {
            GrabReason::Single(s) => s
                .writes
                .iter()
                .map(|c| (s.from, ComponentId::new(c)))
                .collect(),
            GrabReason::Multi(m) => m
                .iter()
                .flat_map(|s| s.writes.iter().map(|c| (s.from, ComponentId::new(c))))
                .collect(),
        }
    }

    pub(crate) fn is_valid(&self, world: &World) -> bool {
        match self {
            GrabReason::Single(a) => a.is_valid(world),
            GrabReason::Multi(accesses) => accesses.iter().all(|a| a.is_valid(world)),
        }
    }
}

/// A world access from a single [`Entity`].
pub struct GrabAccess {
    pub(crate) from: Entity,
    pub(crate) writes: SetUsize,
    pub(crate) reads: SetUsize,
    pub(crate) optional: SetUsize,
}

impl GrabAccess {
    fn is_valid(&self, world: &World) -> bool {
        if let Some(e) = world.get_entity(self.from) {
            for c in self.writes.iter() {
                if !e.contains_id(ComponentId::new(c)) && !self.optional.contains(c) {
                    return false;
                }
            }
            for c in self.reads.iter() {
                if !e.contains_id(ComponentId::new(c)) && !self.optional.contains(c) {
                    return false;
                }
            }

            return true;
        }

        false
    }
}

pub trait GrabParam {
    type Fetch<'w>;

    fn get_access(world: &World, from: Entity) -> GrabAccess;

    fn update_access(world: &World, access: &mut GrabAccess);

    /// Fetch the required data from the world.
    /// # Safety
    /// While the access itself check that no internal conficlts exist, the caller must ensure that
    /// nothing will do an incompatible access at the same time (according to the regular Rust
    /// rules).
    unsafe fn fetch(entity: UnsafeEntityCell<'_>) -> Self::Fetch<'_>;
}

impl<T: Component> GrabParam for &T {
    type Fetch<'w> = Ref<'w, T>;

    fn get_access(world: &World, from: Entity) -> GrabAccess {
        let mut reads = SetUsize::new();
        reads.insert(
            world
                .component_id::<T>()
                .expect("Component should have been initialized")
                .index(),
        );
        GrabAccess {
            from,
            writes: SetUsize::new(),
            reads,
            optional: SetUsize::new(),
        }
    }

    fn update_access(world: &World, access: &mut GrabAccess) {
        let c_id = world
            .component_id::<T>()
            .expect("Component should have been initialized");
        if access.writes.contains(c_id.index()) {
            panic!("Conflicting access detected");
        }
        access.reads.insert(c_id.index());
    }

    unsafe fn fetch(entity: UnsafeEntityCell<'_>) -> Self::Fetch<'_> {
        entity.get_ref::<T>().unwrap()
    }
}

impl<T: Component> GrabParam for &mut T {
    type Fetch<'w> = Mut<'w, T>;

    fn get_access(world: &World, from: Entity) -> GrabAccess {
        let mut writes = SetUsize::new();
        writes.insert(
            world
                .component_id::<T>()
                .expect("Component should have been initialized")
                .index(),
        );
        GrabAccess {
            from,
            writes,
            reads: SetUsize::new(),
            optional: SetUsize::new(),
        }
    }

    fn update_access(world: &World, access: &mut GrabAccess) {
        let c_id = world
            .component_id::<T>()
            .expect("Component should have been initialized");
        if access.reads.contains(c_id.index()) {
            panic!("Conflicting access detected");
        }
        access.writes.insert(c_id.index());
    }

    unsafe fn fetch(entity: UnsafeEntityCell<'_>) -> Self::Fetch<'_> {
        entity.get_mut::<T>().unwrap()
    }
}

impl<T: Component> GrabParam for Option<&T> {
    type Fetch<'w> = Option<&'w T>;

    fn get_access(world: &World, from: Entity) -> GrabAccess {
        let mut res = <&T as GrabParam>::get_access(world, from);
        res.optional.insert(
            world
                .component_id::<T>()
                .expect("Component should have been initialized")
                .index(),
        );
        res
    }

    fn update_access(world: &World, access: &mut GrabAccess) {
        <&T as GrabParam>::update_access(world, access);
        access.optional.insert(
            world
                .component_id::<T>()
                .expect("Component should have been initialized")
                .index(),
        );
    }

    unsafe fn fetch(entity: UnsafeEntityCell<'_>) -> Self::Fetch<'_> {
        entity.get::<T>()
    }
}

impl<T: Component> GrabParam for Option<&mut T> {
    type Fetch<'w> = Option<Mut<'w, T>>;

    fn get_access(world: &World, from: Entity) -> GrabAccess {
        let mut res = <&mut T as GrabParam>::get_access(world, from);
        res.optional.insert(
            world
                .component_id::<T>()
                .expect("Component should have been initialized")
                .index(),
        );
        res
    }

    fn update_access(world: &World, access: &mut GrabAccess) {
        <&mut T as GrabParam>::update_access(world, access);
        access.optional.insert(
            world
                .component_id::<T>()
                .expect("Component should have been initialized")
                .index(),
        );
    }

    unsafe fn fetch(entity: UnsafeEntityCell<'_>) -> Self::Fetch<'_> {
        entity.get_mut::<T>()
    }
}

macro_rules! impl_tuple_grab {
    ($param1: ident, $($param2: ident),*) => {
        impl<$param1: GrabParam, $($param2: GrabParam),+> GrabParam for ($param1, $($param2),*)
        {
            type Fetch<'w> = ($param1::Fetch<'w>, $($param2::Fetch<'w>),*);

            fn get_access(world: &World, from: Entity) -> GrabAccess {
                let mut access = $param1::get_access(world, from);
                $($param2::update_access(world, &mut access));*;
                access
            }

            fn update_access(world: &World, access: &mut GrabAccess) {
                $param1::update_access(world, access);
                $($param2::update_access(world, access));*;
            }

            unsafe fn fetch(entity: UnsafeEntityCell<'_>) -> Self::Fetch<'_> {
                ($param1::fetch(entity), $($param2::fetch(entity)),*)
            }

        }
    };
}

all_tuples!(impl_tuple_grab, 2, 15, G);

#[pin_project]
pub struct GrabCoroutineVoid<'a, P, F>
where
    P: GrabParam,
    F: Future<Output = ()> + Send + Sync,
{
    from: Entity,
    fib: Fib,
    #[pin]
    inner: F,
    _phantom: PhantomData<P>,
    _phantom2: PhantomData<&'a ()>,
}

impl<'a, P, F> GrabCoroutineVoid<'a, P, F>
where
    P: GrabParam,
    F: Future<Output = ()> + Send + Sync,
{
    pub fn new(fib: Fib, from: Entity, inner: F) -> Self {
        GrabCoroutineVoid {
            from,
            fib,
            _phantom: PhantomData,
            inner,
            _phantom2: PhantomData,
        }
    }

    pub fn and<P2: GrabParam>(self, from: Entity) -> GrabCoroutineVoid2<'a, P, P2, F> {
        GrabCoroutineVoid2::new(self.fib, self.from, from, self.inner)
    }
}

/// A wrapper around another coroutine that returns the requested
/// data, once the underlying coroutine has finished.
impl<'a, P, F> Future for GrabCoroutineVoid<'a, P, F>
where
    P: GrabParam,
    F: Future<Output = ()> + Send + Sync,
{
    type Output = P::Fetch<'a>;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        match this.inner.poll(_cx) {
            Poll::Pending => {
                unsafe {
                    let world = this.fib.world_window().world();
                    let access = P::get_access(world, *this.from);
                    this.fib.grab_sender.send(GrabReason::Single(access));
                }
                Poll::Pending
            }
            Poll::Ready(_) => {
                let entity = this.from;

                // Safety: The executor takes care of not executing two conflicting coroutines
                // at the same time.
                unsafe {
                    let world =
                        &mut *this.fib.world_window.get().expect(
                            "This function should have been called when a coroutine is polled",
                        );
                    let world = world.as_unsafe_world_cell();
                    let result = P::fetch(world.get_entity(*entity).unwrap());
                    Poll::Ready(result)
                }
            }
        }
    }
}

// TODO: Generalize with a macro instead
#[pin_project]
pub struct GrabCoroutineVoid2<'a, P1, P2, F>
where
    P1: GrabParam,
    P2: GrabParam,
    F: Future<Output = ()> + Send + Sync,
{
    from1: Entity,
    from2: Entity,
    fib: Fib,
    #[pin]
    inner: F,
    _phantom1: PhantomData<P1>,
    _phantom2: PhantomData<P2>,
    _phantoma: PhantomData<&'a ()>,
}

impl<'a, P1, P2, F> GrabCoroutineVoid2<'a, P1, P2, F>
where
    P1: GrabParam,
    P2: GrabParam,
    F: Future<Output = ()> + Send + Sync,
{
    pub fn new(fib: Fib, from1: Entity, from2: Entity, inner: F) -> Self {
        assert_ne!(from1, from2, "Conficting grab access detected");
        GrabCoroutineVoid2 {
            from1,
            from2,
            fib,
            inner,
            _phantom1: PhantomData,
            _phantom2: PhantomData,
            _phantoma: PhantomData,
        }
    }
}

/// A wrapper around another coroutine that returns the requested
/// data, once the underlying coroutine has finished.
impl<'a, P1, P2, F> Future for GrabCoroutineVoid2<'a, P1, P2, F>
where
    P1: GrabParam,
    P2: GrabParam,
    F: Future<Output = ()> + Send + Sync,
{
    type Output = (P1::Fetch<'a>, P2::Fetch<'a>);

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        match this.inner.poll(_cx) {
            Poll::Pending => {
                unsafe {
                    let world = this.fib.world_window().world();
                    let access1 = P1::get_access(world, *this.from1);
                    let access2 = P2::get_access(world, *this.from2);
                    this.fib
                        .grab_sender
                        .send(GrabReason::Multi(vec![access1, access2]));
                }
                Poll::Pending
            }
            Poll::Ready(_) => {
                let entity1 = this.from1;
                let entity2 = this.from2;
                unsafe {
                    let world =
                        &mut *this.fib.world_window.get().expect(
                            "This function should have been called when a coroutine is polled",
                        );
                    let world = world.as_unsafe_world_cell();
                    let result1 = P1::fetch(world.get_entity(*entity1).unwrap());
                    let result2 = P2::fetch(world.get_entity(*entity2).unwrap());
                    Poll::Ready((result1, result2))
                }
            }
        }
    }
}

/// Same as [`GrabCoroutineVoid`], but where the underlying coroutine
/// can return a value.
#[pin_project]
pub struct GrabCoroutine<'a, P, F, O>
where
    P: GrabParam,
    F: Future<Output = O> + Send + Sync,
{
    from: Entity,
    fib: Fib,
    #[pin]
    inner: F,
    _phantom: PhantomData<P>,
    _phantom2: PhantomData<&'a ()>,
}

//
impl<'a, P, F, O> GrabCoroutine<'a, P, F, O>
where
    P: GrabParam,
    F: Future<Output = O> + Send + Sync,
{
    pub fn new(fib: Fib, from: Entity, inner: F) -> Self {
        GrabCoroutine {
            from,
            fib,
            _phantom: PhantomData,
            inner,
            _phantom2: PhantomData,
        }
    }
}

impl<'a, P, F, O> Future for GrabCoroutine<'a, P, F, O>
where
    P: GrabParam,
    F: Future<Output = O> + Send + Sync,
{
    type Output = (O, P::Fetch<'a>);

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        match this.inner.poll(_cx) {
            Poll::Pending => {
                unsafe {
                    let world = this.fib.world_window().world();
                    let access = P::get_access(world, *this.from);
                    this.fib.grab_sender.send(GrabReason::Single(access));
                }
                Poll::Pending
            }
            Poll::Ready(r) => {
                let entity = this.from;
                unsafe {
                    let world =
                        &mut *this.fib.world_window.get().expect(
                            "This function should have been called when a coroutine is polled",
                        );
                    let world = world.as_unsafe_world_cell();
                    let result = P::fetch(world.get_entity(*entity).unwrap());
                    Poll::Ready((r, result))
                }
            }
        }
    }
}
