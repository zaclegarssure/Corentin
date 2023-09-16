use crate::coroutine::WaitingReason;

use bevy::time::Time;
use bevy::time::Timer;
use bevy::time::TimerMode;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use super::CoroState;
use crate::coroutine::function_coroutine::Fib;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct NextTick<'a> {
    fib: &'a Fib,
    state: CoroState,
}

impl<'a> NextTick<'a> {
    pub(crate) fn new(fib: &'a Fib) -> Self {
        NextTick {
            fib,
            state: CoroState::Running,
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
                    (self.fib.world_window.world_cell())
                        .get_resource::<Time>()
                        .unwrap()
                        .delta()
                };
                Poll::Ready(dt)
            }
            CoroState::Running => {
                self.state = CoroState::Halted;
                self.fib.yield_channel.send(WaitingReason::Tick);
                Poll::Pending
            }
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct DurationFuture<'a> {
    fib: &'a Fib,
    duration: Duration,
    state: CoroState,
}

impl<'a> DurationFuture<'a> {
    pub(crate) fn new(fib: &'a Fib, duration: Duration) -> Self {
        DurationFuture {
            fib,
            duration,
            state: CoroState::Running,
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
                    .yield_channel
                    .send(WaitingReason::Duration(Timer::new(
                        self.duration,
                        TimerMode::Once,
                    )));
                Poll::Pending
            }
        }
    }
}
