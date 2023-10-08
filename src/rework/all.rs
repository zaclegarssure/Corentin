use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use pin_project::pin_project;

use super::{
    handle::HandlerTuple,
    scope::{CoroState, Scope},
    WaitingReason,
};

#[must_use = "futures do nothing unless you `.await` or poll them"]
#[pin_project]
pub struct AwaitAll<'a, H: HandlerTuple> {
    scope: &'a mut Scope,
    handlers: H,
    state: CoroState,
}

impl<'a, H: HandlerTuple> AwaitAll<'a, H> {
    pub(crate) fn new(scope: &'a mut Scope, handlers: H) -> Self {
        AwaitAll {
            scope,
            handlers,
            state: CoroState::Running,
        }
    }
}

impl<H: HandlerTuple> Future for AwaitAll<'_, H> {
    type Output = H::Output;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();
        match this.state {
            // We assume the executor will only poll it once all the coroutines have finish executing
            CoroState::Halted => {
                *this.state = CoroState::Running;
                // Safety: The window is open (we are getting polled) and there can be only
                // one handler to each coroutine (no conflict possible when taking the result).
                unsafe { Poll::Ready(this.handlers.fetch(this.scope)) }
            }
            CoroState::Running => {
                *this.state = CoroState::Halted;
                let set = this.handlers.to_set();
                this.scope.set_waiting_reason(WaitingReason::All(set));
                Poll::Pending
            }
        }
    }
}
