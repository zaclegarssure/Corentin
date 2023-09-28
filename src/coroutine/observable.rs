use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use bevy::{ecs::component::ComponentId, prelude::Entity};

use crate::{coroutine::WaitingReason, prelude::Fib};

use super::CoroState;

/// Anything we can observe in the world will have one specific id
#[derive(Copy, Clone)]
pub enum ObservableId {
    Entity(Entity),
    Component(Entity, ComponentId),
    Resource(ComponentId),
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct OnChange<'a> {
    state: CoroState,
    id: ObservableId,
    fib: &'a mut Fib,
}

impl<'a> OnChange<'a> {
    pub(crate) fn new(fib: &'a mut Fib, id: ObservableId) -> Self {
        Self {
            state: CoroState::Running,
            id,
            fib,
        }
    }
}

impl<'a> Future for OnChange<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.state {
            CoroState::Halted => {
                self.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                self.state = CoroState::Halted;
                self.fib.yield_channel.send(WaitingReason::Changed(self.id));
                Poll::Pending
            }
        }
    }
}
