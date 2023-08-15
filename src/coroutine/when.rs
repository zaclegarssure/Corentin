use bevy::prelude::Component;
use bevy::prelude::Entity;

use crate::coroutine::{CoroState, Fib, WaitingReason};

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Change<'a, T: Component> {
    fib: Fib,
    from: Entity,
    _phantom: PhantomData<T>,
    _phantom2: PhantomData<&'a ()>,
    state: CoroState,
}

impl<'a, T: Component> Change<'a, T> {
    pub(crate) fn new(fib: Fib, from: Entity) -> Self {
        Change {
            fib,
            from,
            _phantom: PhantomData,
            _phantom2: PhantomData,
            state: CoroState::Running,
        }
    }
}

impl<'a, T: Component + Unpin> Future for Change<'a, T> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.state {
            // We assume the executor will only poll it once the component changed
            CoroState::Halted => {
                self.state = CoroState::Running;
                Poll::Ready(())
            }
            CoroState::Running => {
                let c_id = self.fib.component_id::<T>();
                self.state = CoroState::Halted;
                self.fib.yield_sender.send(WaitingReason::Changed {
                    from: self.from,
                    component: c_id,
                });
                Poll::Pending
            }
        }
    }
}

//impl<'a, T: Component> Change<'a, T> {
//    pub fn with<C: 'static>(self) -> ChangeWith<'a, T> {
//        ChangeWith {
//            fib: self.fib,
//            from: self.from,
//            _phantom: PhantomData,
//            with: vec![TypeId::of::<C>()],
//            without: Vec::new(),
//            state: self.state,
//        }
//    }
//
//    pub fn without<C: 'static>(self) -> ChangeWith<'a, T> {
//        ChangeWith {
//            fib: self.fib,
//            from: self.from,
//            _phantom: PhantomData,
//            with: Vec::new(),
//            without: vec![TypeId::of::<C>()],
//            state: self.state,
//        }
//    }
//}
//
//#[must_use = "futures do nothing unless you `.await` or poll them"]
//pub struct ChangeWith<'a, T: Component> {
//    fib: Fib,
//    from: Entity,
//    _phantom: PhantomData<T>,
//    // TODO: make that more efficient and more robust (and not rely on TypeId)
//    with: Vec<TypeId>,
//    without: Vec<TypeId>,
//    state: CoroState,
//    _phantom2: PhantomData<&'a ()>,
//}
//
//impl<'a, T: Component> ChangeWith<'a, T> {
//   pub(crate) fn new(fib: Fib, from: Entity, with:) 
//}
//
//impl<'a, T: Component + Unpin> Future for ChangeWith<'a, T> {
//    type Output = ();
//
//    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
//        match self.state {
//            CoroState::Halted => {
//                self.state = CoroState::Running;
//                Poll::Ready(())
//            }
//            CoroState::Running => {
//                let c_id = self.fib.component_id::<T>();
//                let with: Vec<ComponentId> = self
//                    .with
//                    .iter()
//                    .map(|tid| self.fib.get_component_id(*tid))
//                    .collect();
//                let without: Vec<ComponentId> = self
//                    .without
//                    .iter()
//                    .map(|tid| self.fib.get_component_id(*tid))
//                    .collect();
//                self.state = CoroState::Halted;
//                self.fib.yield_sender.send(WaitingReason::ChangedWith {
//                    from: self.from,
//                    component: c_id,
//                    with,
//                    without,
//                });
//                Poll::Pending
//            }
//        }
//    }
//}
//
//impl<'a, T: Component> ChangeWith<'a, T> {
//    pub fn with<C: 'static>(&mut self) -> &mut Self {
//        self.with.push(TypeId::of::<C>());
//        self
//    }
//
//    pub fn without<C: 'static>(&mut self) -> &mut Self {
//        self.without.push(TypeId::of::<C>());
//        self
//    }
//}
