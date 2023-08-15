use bevy::prelude::Entity;

use crate::coroutine::{CoroState, Fib, WaitingReason};

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use super::grab::GrabCoroutineVoid;
use super::grab::GrabParam;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct ParAnd<'a> {
    fib: Fib,
    coroutines: Vec<Pin<Box<(dyn Future<Output = ()> + 'static + Send + Sync)>>>,
    state: CoroState,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> ParAnd<'a> {
    pub(crate) fn new(
        fib: Fib,
        coroutines: Vec<Pin<Box<(dyn Future<Output = ()> + 'static + Send + Sync)>>>,
    ) -> Self {
        ParAnd {
            fib,
            coroutines,
            state: CoroState::Running,
            _phantom: PhantomData,
        }
    }

    pub fn then_grab<'b, P: GrabParam>(self, from: Entity) -> GrabCoroutineVoid<'a, P, ParAnd<'b>> {
        let fib = self.fib.clone();
        let par_and = ParAnd::new(self.fib, self.coroutines);
        GrabCoroutineVoid::new(fib, from, par_and)
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
                // TODO: Will care about performance later, maybe find a way to inline the coroutines
                // instead of allocating them on the heap ?
                let temp: Vec<Pin<Box<dyn Future<Output = ()> + Send + Sync>>> =
                    self.coroutines.drain(..).collect();
                self.fib
                    .yield_sender
                    .send(WaitingReason::ParAnd { coroutines: temp });
                Poll::Pending
            }
        }
    }
}

impl<'a> ParAnd<'a> {
    /// Add a new coroutine to this [`ParAnd`]. It will have a lower priority than those defined
    /// above.
    pub fn with<C, F>(&mut self, closure: C) -> &mut Self
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib) -> F,
    {
        let fib = self.fib.clone();
        let fut = Box::pin(closure(fib));
        self.coroutines.push(fut);
        self
    }
}
