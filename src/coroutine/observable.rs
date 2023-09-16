use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use bevy::{
    ecs::component::ComponentId,
    prelude::{Component, Entity, Mut, World},
};

use crate::{coroutine::WaitingReason, prelude::Fib, executor::msg_channel::Sender};

use super::{CoroState, coro_param::ParamContext};

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
    context: &'a ParamContext
}


impl<'a> OnChange<'a> {
    pub(crate) fn new(context: &'a ParamContext, id: ObservableId) -> Self {
        Self {
            state: CoroState::Running,
            id,
            context,
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
                self.context.yield_sender.send(WaitingReason::Changed(self.id));
                Poll::Pending
            }
        }
    }
}
