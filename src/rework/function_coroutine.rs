use crate::rework::NewCoroutine;
use bevy::ecs::world::World;

use bevy::prelude::Entity;
use oneshot::Sender;
use std::boxed::Box;
use std::future::Future;

use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use pin_project::pin_project;

use super::coro_param::CoroParam;
use super::CoroAccess;
use super::CoroMeta;

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
    meta: CoroMeta,
    result_sender: Option<Sender<T>>,
}

pub trait CoroutineParamFunction<Marker, T>: Send + 'static {
    type Future: Future<Output = T> + Send + 'static;
    type Params: CoroParam;

    fn init(self, scope: Scope, params: Self::Params) -> Self::Future;
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
    unsafe fn resume_unsafe(
        self: Pin<&mut Self>,
        world: *mut World,
        _ids: &super::id_alloc::Ids,
    ) -> CoroutineResult {
        let waker = waker::create();
        // Dummy context
        let mut cx = Context::from_waker(&waker);

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
                    if let Some(sender) = this.result_sender.take() {
                        let _ = sender.send(t);
                    }

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

    fn meta(&self) -> &CoroMeta {
        &self.meta
    }
}

impl<Marker: 'static, F, T> FunctionCoroutine<Marker, F, T>
where
    T: Send + Sync + 'static,
    F: CoroutineParamFunction<Marker, T>,
{
    pub fn from_function(
        scope: &Scope,
        owner: Entity,
        sender: Option<Sender<T>>,
        f: F,
    ) -> Option<Self> {
        let mut meta = CoroMeta {
            owner,
            access: CoroAccess::default(),
        };

        let params = F::Params::init(scope.world_cell(), &mut meta)?;
        let world_ptr = Box::into_raw(Box::new(std::ptr::null_mut()));
        let ids_ptr = Box::into_raw(Box::new(std::ptr::null()));
        let shared_yield = Box::into_raw(Box::new(None));
        let shared_new_coro = Box::into_raw(Box::default());

        let new_scope = Scope {
            owner,
            ids_ptr,
            world_ptr,
            shared_yield,
            shared_new_coro,
        };
        let future = f.init(new_scope, params);

        Some(Self {
            future,
            world_ptr,
            shared_yield,
            shared_new_coro,
            meta,
            result_sender: sender,
        })
    }
}
