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
use bevy::time::TimerMode;
use pin_project::pin_project;

use crate::CoroObject;

#[derive(PartialEq, Eq)]
pub(crate) enum CoroState {
    Halted,
    Running,
}

pub(crate) enum WaitingReason {
    WaitOnTick,
    WaitOnDuration(Timer),
    WaitOnChange { from: Entity, type_id: TypeId },
    WaitOnParOr { coroutines: Vec<CoroObject> },
}

pub struct Fib {
    pub(crate) state: CoroState,
    // Maybe replace by a real sender receiver channel at some point
    pub(crate) sender: Rc<Cell<Option<WaitingReason>>>,
    pub(crate) owner: Option<Entity>,
}

impl Fib {
    pub fn next_tick<'a>(&'a mut self) -> NextTick<'a> {
        NextTick { fib: self }
    }

    pub fn duration<'a>(&'a mut self, duration: Duration) -> DurationFuture<'a> {
        DurationFuture {
            fib: self,
            duration,
        }
    }

    pub fn change<'a, T: Component + Unpin>(&'a mut self, from: Entity) -> Change<'a, T> {
        Change {
            fib: self,
            from,
            _phantom: PhantomData,
        }
    }

    pub fn par_or<'a, C, F>(&'a mut self, closure: C) -> ParOr<'a>
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.sender),
            owner: self.owner,
        };
        let fut = Box::pin(closure(fib));
        ParOr {
            fib: self,
            coroutines: vec![fut],
        }
    }

    pub fn on<'a, C, F>(&'a mut self, closure: C) -> On<'a, F>
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.sender),
            owner: self.owner,
        };
        let fut = closure(fib);
        On {
            fib: self,
            coroutine: fut,
        }
    }

    pub fn on_self<'a, C, F>(&'a mut self, closure: C) -> On<'a, F>
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib, Entity) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.sender),
            owner: self.owner,
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

    pub fn owner(&self) -> Option<Entity> {
        self.owner
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct NextTick<'a> {
    fib: &'a mut Fib,
}

impl<'a> Future for NextTick<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.fib.state {
            // We assume the executor will only poll it once a new frame has beginned
            CoroState::Halted => {
                self.fib.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                self.fib.state = CoroState::Halted;
                self.fib.sender.replace(Some(WaitingReason::WaitOnTick));
                Poll::Pending
            }
        }
    }
}

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
                    .replace(Some(WaitingReason::WaitOnDuration(Timer::new(
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
                self.fib.sender.replace(Some(WaitingReason::WaitOnChange {
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
    coroutines: Vec<Pin<Box<(dyn Future<Output = ()> + 'static)>>>,
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
                let temp: Vec<Pin<Box<dyn Future<Output = ()>>>> =
                    self.coroutines.drain(..).collect();
                self.fib
                    .sender
                    .replace(Some(WaitingReason::WaitOnParOr { coroutines: temp }));
                Poll::Pending
            }
        }
    }
}

impl<'a> ParOr<'a> {
    pub fn with<C, F>(mut self, closure: C) -> Self
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.fib.sender),
            owner: self.fib.owner,
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
