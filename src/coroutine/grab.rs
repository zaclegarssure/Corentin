use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
    time::Duration,
};

use bevy::{
    ecs::{component::ComponentId, world::unsafe_world_cell::UnsafeEntityCell},
    prelude::{Component, Entity, Mut, Ref, World},
    utils::HashSet,
};
use pin_project::pin_project;


use super::{duration::{DurationFuture, NextTick}, Fib, when::Change};

/// Describe what world access will be perform by a coroutine once resumed
pub(crate) enum GrabReason {
    Single(GrabAccess),
    Multi(Vec<GrabAccess>),
}

/// A world access from a single [`Entity`].
pub struct GrabAccess {
    pub(crate) from: Entity,
    pub(crate) read: HashSet<ComponentId>,
    pub(crate) write: HashSet<ComponentId>,
}

pub trait GrabParam {
    type Fetch<'w>;

    fn get_access<'w>(world: &'w World, from: Entity) -> GrabAccess;

    fn update_access<'w, 'a>(world: &'w World, access: &'a mut GrabAccess);

    fn fetch<'w>(entity: UnsafeEntityCell<'w>) -> Self::Fetch<'w>;
}

impl<T: Component> GrabParam for &T {
    type Fetch<'w> = Ref<'w, T>;

    fn get_access<'w>(world: &'w World, from: Entity) -> GrabAccess {
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

    fn update_access<'w, 'a>(world: &'w World, access: &'a mut GrabAccess) {
        let c_id = world
            .component_id::<T>()
            .expect("Component should have been initialized");
        if access.write.contains(&c_id) {
            panic!("Conflicting access detected");
        }
        access.read.insert(c_id);
    }

    fn fetch<'w>(entity: UnsafeEntityCell<'w>) -> Self::Fetch<'w> {
        // SAFETY: When initialized, we make sure that no conficts exists,
        // and when exexuted (polled) then [`Executor`] makes sure that no
        // other coroutine access that value at the same time.
        unsafe { entity.get_ref::<T>().unwrap() }
    }
}

impl<T: Component> GrabParam for &mut T {
    type Fetch<'w> = Mut<'w, T>;

    fn get_access<'w>(world: &'w World, from: Entity) -> GrabAccess {
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

    fn update_access<'w, 'a>(world: &'w World, access: &'a mut GrabAccess) {
        let c_id = world
            .component_id::<T>()
            .expect("Component should have been initialized");
        if access.read.contains(&c_id) {
            panic!("Conflicting access detected");
        }
        access.write.insert(c_id);
    }

    fn fetch<'w>(entity: UnsafeEntityCell<'w>) -> Self::Fetch<'w> {
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

    fn get_access<'w>(world: &'w World, from: Entity) -> GrabAccess {
        let mut access = G1::get_access(world, from);
        G2::update_access(world, &mut access);
        access
    }

    fn update_access<'w, 'a>(world: &'w World, access: &'a mut GrabAccess) {
        G1::update_access(world, access);
        G2::update_access(world, access);
    }

    fn fetch<'w>(entity: UnsafeEntityCell<'w>) -> Self::Fetch<'w> {
        (G1::fetch(entity), G2::fetch(entity))
    }
}

pub struct GrabFib<'a, P>
where
    P: GrabParam,
{
    from: Entity,
    fib: Fib,
    _phantom: PhantomData<P>,
    _phantom2: PhantomData<&'a ()>,
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
        GrabCoroutineVoid { from, fib, _phantom: PhantomData, inner, _phantom2: PhantomData }
    }
}

/// Similar to [`Fib`], but with the description of which world's data will
/// be accessed once resumed. Not everything is supported since the API
/// will be more convenient later on (and will avoid code duplication)
impl<'a, 'b, P> GrabFib<'a, P>
where
    P: GrabParam,
    'b : 'a,
{
    pub(crate) fn new(fib: Fib, from: Entity) -> Self {
        GrabFib {
            from,
            fib,
            _phantom: PhantomData,
            _phantom2: PhantomData
        }
    }

    /// TODO: Figure out a way to avoid duplication
    /// In particular the API should be like so:
    /// ```ignore
    /// let (dt, (mut transform, health)) = fib.next_tick().and_grab::<(&mut Transform, &Health)>(this);
    ///```
    /// But I had lifetime issues with this...
    pub fn after_duration(
        self,
        duration: Duration,
    ) -> GrabCoroutineVoid<'a, P, DurationFuture<'b>> {
        let d_fut: DurationFuture<'b> = DurationFuture::new(self.fib.clone(), duration);

        GrabCoroutineVoid::new(self.fib.clone(), self.from, d_fut)
    }

    pub fn after_tick(
        self
    ) -> GrabCoroutine<'a, P, NextTick<'b>, Duration> {
        let tick_fut = NextTick::new(self.fib.clone());
        GrabCoroutine::new(self.fib.clone(), self.from, tick_fut)
    }

    pub fn after_change<T: Component + Unpin>(&mut self, from: Entity)
        -> GrabCoroutineVoid<'a, P, Change<'b, T>> {
        let change_fut = Change::new(self.fib.clone(), from);

        GrabCoroutineVoid::new(self.fib.clone(), from, change_fut)
    }

}

/// A wrapper around another coroutine that returns the requested
/// data, once the underlying coroutine has finished.
impl<'a, P, F> Future for GrabCoroutineVoid<'a, P, F>
where
    P: GrabParam,
    F: Future<Output = ()> + Send + Sync + 'static,
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
        GrabCoroutine { from, fib, _phantom: PhantomData, inner, _phantom2: PhantomData }
    }
}


impl<'a, P, F, O> Future for GrabCoroutine<'a, P, F, O>
where
    P: GrabParam,
    F: Future<Output = O> + Send + Sync + 'static,
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
