use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bevy::{
    ecs::{component::ComponentId, world::unsafe_world_cell::UnsafeEntityCell},
    prelude::{Component, Entity, Mut, Ref, World},
    utils::HashSet,
};
use pin_project::pin_project;

use super::Fib;

/// Describe what world access will be perform by a coroutine once resumed
pub(crate) enum GrabReason {
    Single(GrabAccess),
    Multi(Vec<GrabAccess>),
}

//impl GrabReason {
//    pub fn writes(&self) -> impl Iterator<Item = (Entity, ComponentId)> {
//        match self {
//            GrabReason::Single(s) => s.write.iter().map(|c| (s.from, c))
//            GrabReason::Multi(m) => m.iter().flat_map(|s| s.write.iter().map(|c| (s.from, c)))
//        }
//    }
//}

/// A world access from a single [`Entity`].
pub struct GrabAccess {
    pub(crate) from: Entity,
    pub(crate) read: HashSet<ComponentId>,
    pub(crate) write: HashSet<ComponentId>,
}

pub trait GrabParam {
    type Fetch<'w>;

    fn get_access(world: &World, from: Entity) -> GrabAccess;

    fn update_access(world: &World, access: &mut GrabAccess);

    fn fetch(entity: UnsafeEntityCell<'_>) -> Self::Fetch<'_>;
}

impl<T: Component> GrabParam for &T {
    type Fetch<'w> = Ref<'w, T>;

    fn get_access(world: &World, from: Entity) -> GrabAccess {
        let mut read = HashSet::new();
        read.insert(
            world
                .component_id::<T>()
                .expect("Component should have been initialized"),
        );
        GrabAccess {
            from,
            read,
            write: HashSet::new(),
        }
    }

    fn update_access(world: &World, access: &mut GrabAccess) {
        let c_id = world
            .component_id::<T>()
            .expect("Component should have been initialized");
        if access.write.contains(&c_id) {
            panic!("Conflicting access detected");
        }
        access.read.insert(c_id);
    }

    fn fetch(entity: UnsafeEntityCell<'_>) -> Self::Fetch<'_> {
        // SAFETY: When initialized, we make sure that no conficts exists,
        // and when exexuted (polled) then [`Executor`] makes sure that no
        // other coroutine access that value at the same time.
        unsafe { entity.get_ref::<T>().unwrap() }
    }
}

impl<T: Component> GrabParam for &mut T {
    type Fetch<'w> = Mut<'w, T>;

    fn get_access(world: &World, from: Entity) -> GrabAccess {
        let mut write = HashSet::new();
        write.insert(
            world
                .component_id::<T>()
                .expect("Component should have been initialized"),
        );
        GrabAccess {
            from,
            read: HashSet::new(),
            write,
        }
    }

    fn update_access(world: &World, access: &mut GrabAccess) {
        let c_id = world
            .component_id::<T>()
            .expect("Component should have been initialized");
        if access.read.contains(&c_id) {
            panic!("Conflicting access detected");
        }
        access.write.insert(c_id);
    }

    fn fetch(entity: UnsafeEntityCell<'_>) -> Self::Fetch<'_> {
        // SAFETY: When initialized, we make sure that no conficts exists,
        // and when exexuted (polled) then [`Executor`] makes sure that no
        // other coroutine access that value at the same time.
        unsafe { entity.get_mut::<T>().unwrap() }
    }
}

// TODO: Do this with a macro for tuples up to 15
impl<G1, G2> GrabParam for (G1, G2)
where
    G1: GrabParam,
    G2: GrabParam,
{
    type Fetch<'w> = (G1::Fetch<'w>, G2::Fetch<'w>);

    fn get_access(world: &World, from: Entity) -> GrabAccess {
        let mut access = G1::get_access(world, from);
        G2::update_access(world, &mut access);
        access
    }

    fn update_access(world: &World, access: &mut GrabAccess) {
        G1::update_access(world, access);
        G2::update_access(world, access);
    }

    fn fetch(entity: UnsafeEntityCell<'_>) -> Self::Fetch<'_> {
        (G1::fetch(entity), G2::fetch(entity))
    }
}

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
                let world = unsafe {
                    let a =
                        &mut *this.fib.world_window.get().expect(
                            "This function should have been called when a coroutine is polled",
                        );
                    a.as_unsafe_world_cell()
                };
                let result = P::fetch(world.get_entity(*entity).unwrap());
                Poll::Ready(result)
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
                let world = unsafe {
                    let a =
                        &mut *this.fib.world_window.get().expect(
                            "This function should have been called when a coroutine is polled",
                        );
                    a.as_unsafe_world_cell()
                };
                let result1 = P1::fetch(world.get_entity(*entity1).unwrap());
                let result2 = P2::fetch(world.get_entity(*entity2).unwrap());
                Poll::Ready((result1, result2))
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
                let world = unsafe {
                    let a =
                        &mut *this.fib.world_window.get().expect(
                            "This function should have been called when a coroutine is polled",
                        );
                    a.as_unsafe_world_cell()
                };
                let result = P::fetch(world.get_entity(*entity).unwrap());
                Poll::Ready((r, result))
            }
        }
    }
}
