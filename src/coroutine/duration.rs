use crate::coroutine::{CoroState, Fib, WaitingReason};

use bevy::prelude::Entity;
use bevy::time::Time;
use bevy::time::Timer;
use bevy::time::TimerMode;
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;
use std::time::Duration;

use super::grab::GrabCoroutine;
use super::grab::GrabCoroutineVoid;
use super::grab::GrabParam;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct NextTick<'a> {
    fib: Fib,
    state: CoroState,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> NextTick<'a> {
    pub(crate) fn new(fib: Fib) -> Self {
        NextTick {
            fib,
            state: CoroState::Running,
            _phantom: PhantomData,
        }
    }

    pub fn then_grab<'b, P: GrabParam>(
        self,
        from: Entity,
    ) -> GrabCoroutine<'a, P, NextTick<'b>, Duration>
    where
        'b: 'a,
    {
        let fib = self.fib.clone();
        GrabCoroutine::new(fib, from, NextTick::new(self.fib.clone()))
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

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct DurationFuture<'a> {
    fib: Fib,
    duration: Duration,
    state: CoroState,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> DurationFuture<'a> {
    pub(crate) fn new(fib: Fib, duration: Duration) -> Self {
        DurationFuture {
            fib,
            duration,
            state: CoroState::Running,
            _phantom: PhantomData,
        }
    }

    pub fn then_grab<'b, P: GrabParam>(
        self,
        from: Entity,
    ) -> GrabCoroutineVoid<'a, P, DurationFuture<'b>>
    where
        'b: 'a,
    {
        let fib = self.fib.clone();
        GrabCoroutineVoid::new(
            fib,
            from,
            DurationFuture::new(self.fib.clone(), self.duration),
        )
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
