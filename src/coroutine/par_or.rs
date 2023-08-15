use crate::coroutine::{Fib, CoroState, WaitingReason};

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct ParOr<'a> {
    fib: Fib,
    coroutines: Vec<Pin<Box<(dyn Future<Output = ()> + 'static + Send + Sync)>>>,
    state: CoroState,
    _phantom: PhantomData<&'a ()>,
}

impl<'a> ParOr<'a> {
    pub(crate) fn new(
        fib: Fib,
        coroutines: Vec<Pin<Box<(dyn Future<Output = ()> + 'static + Send + Sync)>>>,
    ) -> Self {
        ParOr {
            fib,
            coroutines,
            state: CoroState::Running,
            _phantom: PhantomData
        }
    }
}

impl<'a> Future for ParOr<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.state {
            // We assume the executor will only poll it once one of the coroutine has finished executing
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
                    .send(WaitingReason::ParOr { coroutines: temp });
                Poll::Pending
            }
        }
    }
}

impl<'a> ParOr<'a> {
    /// Add a new coroutine to this [`ParOr`]. It will have a lower priority than those defined
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
