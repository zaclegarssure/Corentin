use crate::coroutine::WaitingReason;

use bevy::time::Time;
use bevy::time::Timer;
use bevy::time::TimerMode;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use super::coro_param::ParamContext;
use super::CoroState;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct NextTick {
    context: ParamContext,
    state: CoroState,
}

impl NextTick {
    pub(crate) fn new(context: ParamContext) -> Self {
        NextTick {
            context,
            state: CoroState::Running,
        }
    }
}

impl Future for NextTick {
    type Output = Duration;

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.state {
            // We assume the executor will only poll it once a new frame has beginned
            CoroState::Halted => {
                self.state = CoroState::Running;

                // SAFETY: None lmao
                let dt = unsafe {
                    (self.context.world_window.world_cell())
                        .get_resource::<Time>()
                        .unwrap()
                        .delta()
                };
                Poll::Ready(dt)
            }
            CoroState::Running => {
                self.state = CoroState::Halted;
                self.context.yield_channel.send(WaitingReason::Tick);
                Poll::Pending
            }
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct DurationFuture {
    context: ParamContext,
    duration: Duration,
    state: CoroState,
}

impl DurationFuture {
    pub(crate) fn new(context: ParamContext, duration: Duration) -> Self {
        DurationFuture {
            context,
            duration,
            state: CoroState::Running,
        }
    }
}

impl Future for DurationFuture {
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
                self.context
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
