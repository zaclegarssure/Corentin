use crate::coroutine::{CoroState, Fib, WaitingReason};

use bevy::time::Time;
use bevy::time::Timer;
use bevy::time::TimerMode;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use super::Primitive;
use super::PrimitiveVoid;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct NextTick<'a> {
    fib: &'a Fib,
    state: CoroState,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> NextTick<'a> {
    pub(crate) fn new(fib: &'a Fib) -> Self {
        NextTick {
            fib,
            state: CoroState::Running,
            _phantom: PhantomData,
        }
    }
}

impl<'a> Future for NextTick<'a> {
    type Output = Duration;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.state {
            // We assume the executor will only poll it once a new frame has beginned
            CoroState::Halted => {
                self.state = CoroState::Running;

                // SAFETY: None lmao
                let dt = unsafe {
                    (*self.fib.world_window.get().unwrap())
                        .resource::<Time>()
                        .delta()
                };
                Poll::Ready(dt)
            }
            CoroState::Running => {
                self.state = CoroState::Halted;
                self.fib.yield_sender.send(WaitingReason::Tick);
                Poll::Pending
            }
        }
    }
}


impl<'cx> Primitive<'cx, Duration> for NextTick<'cx> {
    fn get_context(&self) -> &Fib {
        &self.fib
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct DurationFuture<'a> {
    fib: &'a Fib,
    duration: Duration,
    state: CoroState,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> DurationFuture<'a> {
    pub(crate) fn new(fib: &'a Fib, duration: Duration) -> Self {
        DurationFuture {
            fib,
            duration,
            state: CoroState::Running,
            _phantom: PhantomData,
        }
    }

}

impl<'a> Future for DurationFuture<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.state {
            // We assume the executor will only poll it once the duration is over
            CoroState::Halted => {
                self.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                self.state = CoroState::Halted;
                self.fib
                    .yield_sender
                    .send(WaitingReason::Duration(Timer::new(
                        self.duration,
                        TimerMode::Once,
                    )));
                Poll::Pending
            }
        }
    }
}

impl<'cx> PrimitiveVoid<'cx> for DurationFuture<'cx> {
    fn get_context(&self) -> &Fib {
        &self.fib
    }
}
