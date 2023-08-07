use std::any::TypeId;
use std::cell::Cell;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use bevy::ecs::component::ComponentId;
use bevy::prelude::Component;
use bevy::prelude::Entity;
use bevy::prelude::Timer;
use bevy::prelude::World;
use bevy::time::TimerMode;

pub(crate) enum CoroState {
    Halted,
    Running,
}

#[derive(Component)]
pub enum WaitingState {
    WaitOnTick,
    WaitOnDuration(Timer),
    // TODO: should be a relation
    WaitOnChange {
        from: Entity,
        component_id: ComponentId,
    },
    Ready,
}

pub(crate) enum WaitingReason {
    WaitOnTick,
    WaitOnDuration(Timer),
    WaitOnChange { from: Entity, type_id: TypeId },
}

impl WaitingState {
    pub(crate) fn from_reason(value: WaitingReason, world: &World) -> Self {
        match value {
            WaitingReason::WaitOnTick => WaitingState::WaitOnTick,
            WaitingReason::WaitOnDuration(d) => WaitingState::WaitOnDuration(d),
            WaitingReason::WaitOnChange { from, type_id } => WaitingState::WaitOnChange {
                from,
                component_id: world.components().get_id(type_id).unwrap(),
            },
        }
    }
}

// TODO: Should be a relation
#[derive(Component)]
pub struct OwnedBy(pub Entity);

pub struct Fib {
    pub(crate) state: CoroState,
    // Maybe replace by a real sender receiver channel at some point
    pub(crate) sender: Rc<Cell<Option<WaitingReason>>>,
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
