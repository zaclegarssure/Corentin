use std::any::TypeId;
use std::cell::Cell;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use bevy::prelude::Component;
use bevy::prelude::Entity;
use bevy::prelude::Timer;
use bevy::prelude::World;
use bevy::time::Time;
use bevy::time::TimerMode;
use pin_project::pin_project;

use crate::executor::CoroObject;

#[derive(PartialEq, Eq, Clone, Copy)]
pub(crate) enum CoroState {
    Halted,
    Running,
}

pub(crate) enum WaitingReason {
    Tick,
    Duration(Timer),
    Change { from: Entity, type_id: TypeId },
    ParOr { coroutines: Vec<CoroObject> },
    ParAnd { coroutines: Vec<CoroObject> },
}

/// A "Fiber" object, througth which a coroutine
/// can interact with the rest of the world.
#[derive(Clone)]
pub struct Fib {
    pub(crate) state: CoroState,
    // Maybe replace by a real sender receiver channel at some point
    pub(crate) sender: Rc<Cell<Option<WaitingReason>>>,
    pub(crate) owner: Option<Entity>,
    pub(crate) world_window: Rc<Cell<Option<*mut World>>>,
}

// SAFETY: Same as Executor, the sender field is only accessed when polled,
// which is done in a single threaded context.
unsafe impl Send for Fib {}
unsafe impl Sync for Fib {}

impl Fib {
    /// Returns coroutine that resolve the next time the [`Executor`] is ticked (via
    /// [`run`][crate::executor::Executor::run] for instance). It returns the duration
    /// of the last frame (delta time).
    ///
    /// [`Executor`]: crate::executor::Executor
    pub fn next_tick(&mut self) -> NextTick<'_> {
        NextTick { fib: self }
    }

    /// Returns a coroutine that resolve after a certain [`Duration`]. Note that if the duration
    /// is smaller than the time between two tick of the [`Executor`] it won't be compensated.
    ///
    /// [`Executor`]: crate::executor::Executor
    pub fn duration(&mut self, duration: Duration) -> DurationFuture<'_> {
        DurationFuture {
            fib: self,
            duration,
        }
    }

    /// Returns a coroutine that resolve once the [`Component`] of type `T` from a particular
    /// [`Entity`] has changed. Note that if the entity or the components is removed,
    /// this coroutine is dropped.
    pub fn change<T: Component + Unpin>(&mut self, from: Entity) -> Change<'_, T> {
        Change {
            fib: self,
            from,
            _phantom: PhantomData,
        }
    }

    /// Returns a coroutine that resolve once any of the underlying coroutine finishes. Note that
    /// once this is done, all the others are dropped. The coroutines are resumed from top to
    /// bottom, in case multiple of them are ready to make progress at the same time.
    pub fn par_or<C, F>(&mut self, closure: C) -> ParOr<'_>
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.sender),
            owner: self.owner,
            world_window: Rc::clone(&self.world_window),
        };
        let fut = Box::pin(closure(fib));
        ParOr {
            fib: self,
            coroutines: vec![fut],
        }
    }

    /// Returns a coroutine that resolve once the underlying coroutine finishes,
    /// in order to reuse coroutines, because the following won't compile:
    /// ```compile_fail
    ///# use corentin::prelude::*;
    ///async fn sub_coro(mut fib: Fib) { }
    ///async fn main_coro(mut fib: Fib) {
    ///  sub_coro(fib).await;
    ///  sub_coro(fib).await;
    ///}
    ///```
    /// But the following will:
    ///```
    ///# use corentin::prelude::*;
    ///async fn sub_coro(mut fib: Fib) { }
    ///async fn main_coro(mut fib: Fib) {
    ///  fib.on(sub_coro).await;
    ///  fib.on(sub_coro).await;
    ///}
    ///```
    pub fn on<C, F>(&mut self, closure: C) -> On<'_, F>
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.sender),
            owner: self.owner,
            world_window: Rc::clone(&self.world_window),
        };
        let fut = closure(fib);
        On {
            fib: self,
            coroutine: fut,
        }
    }

    /// Same as [`Self::on()`] but with a coroutine that expect
    /// an owner entity as a parameter.
    pub fn on_self<C, F>(&mut self, closure: C) -> On<'_, F>
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib, Entity) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.sender),
            owner: self.owner,
            world_window: Rc::clone(&self.world_window),
        };
        let fut = closure(
            fib,
            self.owner
                .expect("Cannot call on_self if the coroutine is not owned by any entity."),
        );
        On {
            fib: self,
            coroutine: fut,
        }
    }

    /// Returns a coroutine that resolve once all of the underlying coroutine finishes. The
    /// coroutines are resumed from top to bottom, in case multiple of them are ready to make
    /// progress at the same time.
    pub fn par_and<C, F>(&mut self, closure: C) -> ParAnd<'_>
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.sender),
            owner: self.owner,
            world_window: Rc::clone(&self.world_window),
        };
        let fut = Box::pin(closure(fib));
        ParAnd {
            fib: self,
            coroutines: vec![fut],
        }
    }

    pub fn owner(&self) -> Option<Entity> {
        self.owner
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct NextTick<'a> {
    fib: &'a mut Fib,
}

impl<'a> Future for NextTick<'a> {
    type Output = Duration;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.fib.state {
            // We assume the executor will only poll it once a new frame has beginned
            CoroState::Halted => {
                self.fib.state = CoroState::Running;

                // SAFETY: None lmao
                let dt = unsafe {
                    (*self.fib.world_window.get().unwrap()).resource::<Time>().delta()
                };
                Poll::Ready(dt)
            }
            CoroState::Running => {
                self.fib.state = CoroState::Halted;
                self.fib.sender.replace(Some(WaitingReason::Tick));
                Poll::Pending
            }
        }
    }
}

//impl<'a> NextTick<'a> {
//    fn and_request<T: Component>(mut self, from: Entity) -> Requesting<'a, T, Self, Duration> {
//        let fib = Fib::clone(self.fib);
//        Requesting {
//            coroutine: self,
//            fib: fib,
//            from,
//            _phantom: PhantomData,
//        }
//    }
//}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct DurationFuture<'a> {
    fib: &'a mut Fib,
    duration: Duration,
}

impl<'a> Future for DurationFuture<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.fib.state {
            // We assume the executor will only poll it once the duration is over
            CoroState::Halted => {
                self.fib.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                self.fib.state = CoroState::Halted;
                self.fib
                    .sender
                    .replace(Some(WaitingReason::Duration(Timer::new(
                        self.duration,
                        TimerMode::Once,
                    ))));
                Poll::Pending
            }
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Change<'a, T: Component + Unpin> {
    fib: &'a mut Fib,
    from: Entity,
    _phantom: PhantomData<T>,
}

impl<'a, T: Component + Unpin> Future for Change<'a, T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.fib.state {
            // We assume the executor will only poll it once the component changed
            CoroState::Halted => {
                self.fib.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                self.fib.state = CoroState::Halted;
                self.fib.sender.replace(Some(WaitingReason::Change {
                    from: self.from,
                    type_id: TypeId::of::<T>(),
                }));
                Poll::Pending
            }
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct ParOr<'a> {
    fib: &'a mut Fib,
    coroutines: Vec<Pin<Box<(dyn Future<Output = ()> + 'static + Send + Sync)>>>,
}

impl<'a> Future for ParOr<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.fib.state {
            // We assume the executor will only poll it once one of the coroutine has finished executing
            CoroState::Halted => {
                self.fib.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                self.fib.state = CoroState::Halted;
                // TODO: Will care about performance later, maybe find a way to inline the coroutines
                // instead of allocating them on the heap ?
                let temp: Vec<Pin<Box<dyn Future<Output = ()> + Send + Sync>>> =
                    self.coroutines.drain(..).collect();
                self.fib
                    .sender
                    .replace(Some(WaitingReason::ParOr { coroutines: temp }));
                Poll::Pending
            }
        }
    }
}

impl<'a> ParOr<'a> {
    /// Add a new coroutine to this [`ParOr`]. It will have a lower priority than those defined
    /// above.
    pub fn with<C, F>(&mut self, closure: C) -> &mut Self
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.fib.sender),
            owner: self.fib.owner,
            world_window: Rc::clone(&self.fib.world_window),
        };
        let fut = Box::pin(closure(fib));
        self.coroutines.push(fut);
        self
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct ParAnd<'a> {
    fib: &'a mut Fib,
    coroutines: Vec<Pin<Box<(dyn Future<Output = ()> + 'static + Send + Sync)>>>,
}

impl<'a> Future for ParAnd<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.fib.state {
            // We assume the executor will only poll it once all the coroutines have finish executing
            CoroState::Halted => {
                self.fib.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                self.fib.state = CoroState::Halted;
                // TODO: Will care about performance later, maybe find a way to inline the coroutines
                // instead of allocating them on the heap ?
                let temp: Vec<Pin<Box<dyn Future<Output = ()> + Send + Sync>>> =
                    self.coroutines.drain(..).collect();
                self.fib
                    .sender
                    .replace(Some(WaitingReason::ParAnd { coroutines: temp }));
                Poll::Pending
            }
        }
    }
}

impl<'a> ParAnd<'a> {
    /// Add a new coroutine to this [`ParAnd`]. It will have a lower priority than those defined
    /// above.
    pub fn with<C, F>(&mut self, closure: C) -> &mut Self
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.fib.sender),
            owner: self.fib.owner,
            world_window: Rc::clone(&self.fib.world_window),
        };
        let fut = Box::pin(closure(fib));
        self.coroutines.push(fut);
        self
    }
}

#[pin_project]
pub struct On<'a, F>
where
    F: Future<Output = ()> + 'static,
{
    fib: &'a mut Fib,
    #[pin]
    coroutine: F,
}

impl<'a, F> Future for On<'a, F>
where
    F: Future<Output = ()> + 'static,
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();

        if this.fib.state == CoroState::Running {
            this.fib.state = CoroState::Halted;
        }

        match this.coroutine.poll(_cx) {
            Poll::Ready(_) => {
                this.fib.state = CoroState::Running;
                Poll::Ready(())
            }
            Poll::Pending => {
                this.fib.state = CoroState::Halted;
                Poll::Pending
            }
        }
    }
}

//#[pin_project]
//pub struct Requesting<'a, T, F, O> 
//where
//    T: Component,
//    F: Future<Output = O> + 'static, {
//    #[pin]
//    coroutine: F,
//    fib: &'a mut Fib,
//    from: Entity,
//    _phantom: PhantomData<&'a T>,
//}
//
//impl<'a, T, F, O> Future for Requesting<'a, T, F, O>
//where
//    T: Component,
//    F: Future<Output = O> + 'static,
//{
//    type Output = (O, bevy::prelude::Ref<'a, T>);
//
//    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
//        let this = self.project();
//        match this.coroutine.poll(_cx) {
//            Poll::Pending => Poll::Pending,
//            Poll::Ready(o) => {
//                // SAFETY: It's like pretty late right now and I'm tired so idk...
//                let component = unsafe {
//                    (*this.fib.world_window.get().unwrap()).entity(*this.from).get_ref::<T>().unwrap()
//                };
//                Poll::Ready((o, component))
//            }
//        }
//    }
//}
