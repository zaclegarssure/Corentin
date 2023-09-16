//use bevy::prelude::Component;
//use bevy::prelude::Entity;
//
//use crate::coroutine::{CoroState, WaitingReason};
//
//use std::future::Future;
//use std::marker::PhantomData;
//use std::pin::Pin;
//use std::task::Context;
//use std::task::Poll;
//
//
//#[must_use = "futures do nothing unless you `.await` or poll them"]
//pub struct Change<'a, T: Component> {
//    fib: &'a Fib,
//    from: Entity,
//    _phantom: PhantomData<T>,
//    state: CoroState,
//}
//
//impl<'a, T: Component + Unpin> Change<'a, T> {
//    pub(crate) fn new(fib: &'a Fib, from: Entity) -> Self {
//        Change {
//            fib,
//            from,
//            _phantom: PhantomData,
//            state: CoroState::Running,
//        }
//    }
//}
//
//impl<'a, T: Component + Unpin> Future for Change<'a, T> {
//    type Output = ();
//
//    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
//        match self.state {
//            // We assume the executor will only poll it once the component changed
//            CoroState::Halted => {
//                self.state = CoroState::Running;
//                Poll::Ready(())
//            }
//            CoroState::Running => {
//                let c_id = self.fib.component_id::<T>();
//                self.state = CoroState::Halted;
//                self.fib.yield_sender.send(WaitingReason::Changed {
//                    from: self.from,
//                    component: c_id,
//                });
//                Poll::Pending
//            }
//        }
//    }
//}
