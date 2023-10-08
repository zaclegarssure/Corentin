use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use pin_project::pin_project;
use tinyset::SetU64;

use super::{
    handle::CoroHandle,
    scope::{CoroState, Scope},
    WaitingReason,
};

#[must_use = "futures do nothing unless you `.await` or poll them"]
#[pin_project]
pub struct AwaitFirst<'a, const N: usize, T> {
    scope: &'a mut Scope,
    handles: [CoroHandle<T>; N],
    state: CoroState,
}

impl<'a, const N: usize, T> AwaitFirst<'a, N, T> {
    pub(crate) fn new(scope: &'a mut Scope, handles: [CoroHandle<T>; N]) -> Self {
        Self {
            scope,
            handles,
            state: CoroState::Running,
        }
    }
}

impl<const N: usize, T: Send + Sync + 'static> Future for AwaitFirst<'_, N, T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        let this = self.project();
        match this.state {
            // We assume the executor will only poll it once all the coroutines have finish executing
            CoroState::Halted => {
                *this.state = CoroState::Running;
                // Safety: The window is open (we are getting polled) and there can be only
                // one handle to each coroutine (no conflict possible when taking the result).
                unsafe {
                    for handle in this.handles.iter() {
                        if let Some(t) = handle.try_fetch(this.scope) {
                            return Poll::Ready(t);
                        }
                    }
                }
                panic!("The executor resumed a coroutine at the wrong time, this is a bug");
            }
            CoroState::Running => {
                *this.state = CoroState::Halted;
                let mut set = SetU64::new();
                for h in this.handles.iter() {
                    set.insert(h.id.to_bits());
                }
                this.scope.set_waiting_reason(WaitingReason::First(set));
                Poll::Pending
            }
        }
    }
}
