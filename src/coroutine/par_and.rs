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
    fib: &'a mut Fib,
    coroutines: Vec<CoroObject>,
    state: CoroState,
}

impl<'a> ParAnd<'a> {
    pub(crate) fn new(fib: &'a mut Fib) -> Self {
        ParAnd {
            fib,
            coroutines: vec![],
            state: CoroState::Running,
        }
    }

    /// Add a new coroutine to this [`ParAnd`].
    pub fn with<C, Marker>(mut self, coro: C) -> Self
    where
        C: UninitCoroutine<Marker>,
    {
        // Safety: We are getting polled right now, therefore we have exclusive world access.
        unsafe {
            if let Some(c) = coro.init(
                self.fib.owner,
                self.fib.world_window.world_cell().world_mut(),
            ) {
                self.coroutines.push(SyncCell::new(Box::pin(c)));
            }
        }
        self
    }
}

impl Future for ParAnd<'_> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.state {
            // We assume the executor will only poll it once all the coroutines have finish executing
            CoroState::Halted => {
                self.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                if self.coroutines.is_empty() {
                    return Poll::Ready(());
                }
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
