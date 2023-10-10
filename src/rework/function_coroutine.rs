use crate::rework::NewCoroutine;
use bevy::ecs::world::World;
use oneshot::Sender;
use std::future::Future;
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use pin_project::pin_project;

use super::id_alloc::Id;
use super::scope::Scope;
use super::waker;
use super::Coroutine;
use super::CoroutineResult;
use super::CoroutineStatus;
use super::WaitingReason;

#[pin_project]
pub struct FunctionCoroutine<Marker, F, T>
where
    F: CoroutineParamFunction<Marker, T>,
{
    #[pin]
    future: F::Future,
    world_ptr: *mut *mut World,
    shared_yield: *mut Option<WaitingReason>,
    shared_new_coro: *mut Vec<NewCoroutine>,
    result_sender: MaybeUninit<Sender<T>>,
    id: Id,
}

pub trait CoroutineParamFunction<Marker, T>: Send + 'static {
    type Future: Future<Output = T> + Send + 'static;

    fn init(self, scope: Scope) -> Self::Future;
}

// Safety: ???
unsafe impl<Marker, F, T> Send for FunctionCoroutine<Marker, F, T> where
    F: CoroutineParamFunction<Marker, T>
{
}

const ERR_WRONGAWAIT: &str = "A coroutine yielded without notifying the executor
the reason. That is most likely because it awaits a
future which is not part of this library.";

impl<Marker: 'static, F, T> Coroutine for FunctionCoroutine<Marker, F, T>
where
    T: Send + Sync + 'static,
    F: CoroutineParamFunction<Marker, T>,
{
    fn resume(self: Pin<&mut Self>, world: &mut World) -> super::CoroutineResult {
        let waker = waker::create();
        // Dummy context
        let mut cx = Context::from_waker(&waker);

        let world = world as *mut _;

        let this = self.project();

        // Safety: Mec crois moi
        unsafe {
            **this.world_ptr = world;
            let res = this.future.poll(&mut cx);
            **this.world_ptr = std::ptr::null_mut();

            let mut result = CoroutineResult {
                result: CoroutineStatus::Done,
                new_coro: std::mem::take(&mut **this.shared_new_coro),
            };

            match res {
                Poll::Ready(t) => {
                    this.result_sender.assume_init_read().send(t);

                    drop(Box::from_raw(*this.shared_yield));
                    drop(Box::from_raw(*this.shared_new_coro));
                    result
                }
                Poll::Pending => {
                    result.result =
                        CoroutineStatus::Yield((**this.shared_yield).take().expect(ERR_WRONGAWAIT));
                    result
                }
            }
        }
    }

    fn is_valid(&self, _world: &World) -> bool {
        true
    }
}
