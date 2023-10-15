use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::rework::executor::msg::{CoroStatus, SignalId};

use super::{scope::Scope, CoroState};

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct AwaitChange<'a> {
    scope: &'a mut Scope,
    id: SignalId,
    state: CoroState,
}

impl<'a> AwaitChange<'a> {
    pub fn new(scope: &'a mut Scope, id: SignalId) -> Self {
        Self {
            scope,
            id,
            state: CoroState::Running,
        }
    }
}

impl<'a> Future for AwaitChange<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.state {
            CoroState::Halted => Poll::Ready(()),
            CoroState::Running => {
                self.state = CoroState::Halted;

                let id = self.id;
                self.scope.yield_(CoroStatus::Signal(id));
                Poll::Pending
            }
        }
    }
}
