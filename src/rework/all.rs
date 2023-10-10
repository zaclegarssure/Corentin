use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use pin_project::pin_project;

use super::{
    handle::{HandleTuple, Status},
    scope::{CoroState, Scope},
    WaitingReason,
};

#[must_use = "futures do nothing unless you `.await` or poll them"]
#[pin_project]
pub struct AwaitAll<'a, H: HandleTuple> {
    scope: &'a mut Scope,
    handlers: H,
    state: CoroState,
}

impl<'a, H: HandleTuple> AwaitAll<'a, H> {
    pub(crate) fn new(scope: &'a mut Scope, handlers: H) -> Self {
        AwaitAll {
            scope,
            handlers,
            state: CoroState::Running,
        }
    }
}

impl<H: HandleTuple> Future for AwaitAll<'_, H> {
    type Output = H::Output;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();
        match this.state {
            // We assume the executor will only poll it once all the coroutines have finish executing
            CoroState::Halted => {
                *this.state = CoroState::Running;

                match this.handlers.update_status() {
                    Status::Done => Poll::Ready(this.handlers.try_fetch().unwrap()),
                    // The executor should only poll when all is ready
                    _ => unreachable!(),
                }
            }
            CoroState::Running => {
                *this.state = CoroState::Halted;
                match this.handlers.update_status() {
                    Status::Done => Poll::Ready(this.handlers.try_fetch().unwrap()),
                    Status::StillWaiting(ids) => {
                        this.scope.set_waiting_reason(WaitingReason::All(ids));
                        Poll::Pending
                    }
                    _ => {
                        this.scope.set_waiting_reason(WaitingReason::Cancel);
                        Poll::Pending
                    }
                }
            }
        }
    }
}
