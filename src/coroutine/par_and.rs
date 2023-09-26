use bevy::utils::synccell::SyncCell;

use crate::coroutine::{CoroState, WaitingReason};
use crate::prelude::Fib;

use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use super::{CoroObject, UninitCoroutine};

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct ParAnd<'a> {
    fib: &'a Fib,
    coroutines: Vec<CoroObject>,
    state: CoroState,
}

impl<'a> ParAnd<'a> {
    pub(crate) fn new(fib: &'a Fib, coroutines: Vec<CoroObject>) -> Self {
        ParAnd {
            fib,
            coroutines,
            state: CoroState::Running,
        }
    }
}

impl<'a> Future for ParAnd<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.state {
            // We assume the executor will only poll it once all the coroutines have finish executing
            CoroState::Halted => {
                self.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                self.state = CoroState::Halted;
                let coroutines = std::mem::take(&mut self.coroutines);
                self.fib
                    .yield_channel
                    .send(WaitingReason::ParAnd { coroutines });
                Poll::Pending
            }
        }
    }
}

impl<'a> ParAnd<'a> {
    /// Add a new coroutine to this [`ParAnd`].
    pub fn with<C, Marker>(&mut self, coro: C) -> &mut Self
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
