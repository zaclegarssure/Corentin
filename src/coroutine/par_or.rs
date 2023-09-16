use bevy::utils::synccell::SyncCell;

use crate::coroutine::{CoroState, WaitingReason};

use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use super::function_coroutine::Fib;
use super::CoroObject;
use super::UninitCoroutine;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct ParOr<'a> {
    fib: &'a Fib,
    coroutines: Vec<CoroObject>,
    state: CoroState,
}

impl<'a> ParOr<'a> {
    pub(crate) fn new(fib: &'a Fib, coroutines: Vec<CoroObject>) -> Self {
        ParOr {
            fib,
            coroutines,
            state: CoroState::Running,
        }
    }
}

impl<'a> Future for ParOr<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.state {
            // We assume the executor will only poll it once one of the coroutine has finished executing
            CoroState::Halted => {
                self.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                self.state = CoroState::Halted;
                let coroutines = std::mem::take(&mut self.coroutines);
                self.fib
                    .yield_channel
                    .send(WaitingReason::ParOr { coroutines });
                Poll::Pending
            }
        }
    }
}

impl<'a> ParOr<'a> {
    /// Add a new coroutine to this [`ParOr`].
    pub fn with<C, Marker: 'static>(&mut self, coro: C) -> &mut Self
    where
        C: UninitCoroutine<Marker>,
    {
        // Safety: We are getting polled right now, therefore we have exclusive world access.
        unsafe {
            if let Some(c) = coro.init(
                self.fib.meta.owner,
                self.fib.world_window.world_cell().world_mut(),
            ) {
                self.coroutines.push(SyncCell::new(Box::pin(c)));
            }
        }
        self
    }
}
