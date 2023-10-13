use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use pin_project::pin_project;
use tinyset::SetU64;

use super::{
    handle::{CoroHandle, HandleTuple, Status},
    CoroState, CoroStatus, Scope,
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
            // We assume the executor will only poll it once any of the coroutines have finish executing
            CoroState::Halted => {
                *this.state = CoroState::Running;
                for h in this.handles {
                    if let Status::Done = h.update_status() {
                        return Poll::Ready(h.try_fetch().unwrap());
                    }
                }
                panic!("The executor resumed a coroutine at the wrong time, this is a bug");
            }
            CoroState::Running => {
                *this.state = CoroState::Halted;
                let mut set = SetU64::new();
                let mut done = None;
                for h in this.handles {
                    match h.update_status() {
                        Status::Done => {
                            if let None = done {
                                done = Some(h.try_fetch().unwrap());
                            }
                        }
                        Status::StillWaiting(id) => {
                            set.extend(id);
                        }
                        _ => {
                            this.scope.yield_(CoroStatus::Cancel);
                            return Poll::Pending;
                        }
                    }
                }
                if let Some(value) = done {
                    return Poll::Ready(value);
                }
                this.scope.yield_(CoroStatus::First(set));
                Poll::Pending
            }
        }
    }
}
