use bevy::time::Time;
use bevy::time::Timer;
use bevy::time::TimerMode;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use super::scope::CoroState;
use super::scope::Scope;
use super::CoroStatus;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct NextTick<'a> {
    scope: &'a mut Scope,
    state: CoroState,
}

impl<'a> NextTick<'a> {
    pub fn new(scope: &'a mut Scope) -> Self {
        NextTick {
            scope,
            state: CoroState::Running,
        }
    }
}

impl Future for NextTick<'_> {
    type Output = Duration;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.state {
            // We assume the executor will only poll it once a new frame has beginned
            CoroState::Halted => {
                self.state = CoroState::Running;

                // SAFETY: See [`Executor`]
                let dt = unsafe {
                    (self.scope.world_cell())
                        .get_resource::<Time>()
                        .unwrap()
                        .delta()
                };
                Poll::Ready(dt)
            }
            CoroState::Running => {
                self.state = CoroState::Halted;
                self.scope.yield_(CoroStatus::Tick);
                Poll::Pending
            }
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct DurationFuture<'a> {
    scope: &'a mut Scope,
    duration: Duration,
    state: CoroState,
}

impl<'a> DurationFuture<'a> {
    pub fn new(scope: &'a mut Scope, duration: Duration) -> Self {
        DurationFuture {
            scope,
            duration,
            state: CoroState::Running,
        }
    }
}

impl Future for DurationFuture<'_> {
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
                let status = CoroStatus::Duration(Timer::new(self.duration, TimerMode::Once));
                self.scope.yield_(status);
                Poll::Pending
            }
        }
    }
}
