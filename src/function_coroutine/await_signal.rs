use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use pin_project::pin_project;

use crate::{
    coro_param::signals::{Signal, SignalType},
    executor::msg::{CoroStatus, SignalId},
};

use super::{scope::Scope, CoroState};

#[must_use = "futures do nothing unless you `.await` or poll them"]
#[pin_project]
pub struct AwaitSignal<'a, S, T> {
    scope: &'a mut Scope,
    id: SignalId,
    state: CoroState,
    _phantom1: PhantomData<S>,
    _phantom2: PhantomData<T>,
}

impl<'a, S, T> AwaitSignal<'a, S, T> {
    pub fn new(scope: &'a mut Scope, id: SignalId) -> Self {
        AwaitSignal {
            scope,
            id,
            state: CoroState::Running,
            _phantom1: PhantomData,
            _phantom2: PhantomData,
        }
    }
}

impl<'a, S, T> Future for AwaitSignal<'a, S, T>
where
    S: SignalType<T> + Send + Sync + 'static,
    T: Copy + Send + Sync + 'static,
{
    type Output = T;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match this.state {
            CoroState::Halted => {
                *this.state = CoroState::Running;

                let t = match this.id.owner {
                    Some(entity) => unsafe {
                        this.scope
                            .resume_param_mut()
                            .world_cell()
                            .get_entity(entity)
                            .unwrap()
                            .get_ref::<Signal<S, T>>()
                            .unwrap()
                            .value
                            .read()
                    },
                    None => unsafe {
                        this.scope
                            .resume_param_mut()
                            .world_cell()
                            .get_resource::<Signal<S, T>>()
                            .unwrap()
                            .value
                            .read()
                    },
                };

                Poll::Ready(t)
            }
            CoroState::Running => {
                this.scope.yield_(CoroStatus::Tick);
                Poll::Pending
            }
        }
    }
}
