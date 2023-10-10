use crate::rework::NewCoroutine;
use bevy::ecs::world::World;

use bevy::prelude::Entity;
use bevy::utils::all_tuples;
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

use super::id_alloc::Ids;
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
    ids_ptr: *mut *const Ids,
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
    unsafe fn resume_unsafe(self: Pin<&mut Self>, world: *mut World, ids: &Ids) -> CoroutineResult {
        let waker = waker::create();
        // Dummy context
        let mut cx = Context::from_waker(&waker);

        let this = self.project();

        // Safety: Mec crois moi
        unsafe {
            **this.world_ptr = world;
            **this.ids_ptr = ids;
            let res = this.future.poll(&mut cx);
            **this.world_ptr = std::ptr::null_mut();
            **this.ids_ptr = std::ptr::null();

            let mut result = CoroutineResult {
                result: CoroutineStatus::Done,
                new_coro: std::mem::take(&mut **this.shared_new_coro),
            };

            match res {
                Poll::Ready(t) => {
                    if let Some(sender) = this.result_sender.take() {
                        let _ = sender.send(t);
                    }

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

    fn cleanup(&self) {
        unsafe {
            drop(Box::from_raw(self.shared_yield));
            drop(Box::from_raw(self.shared_new_coro));
            drop(Box::from_raw(self.world_ptr));
            drop(Box::from_raw(self.ids_ptr));
        }
    }
}

impl<Marker: 'static, F, T> FunctionCoroutine<Marker, F, T>
where
    T: Send + Sync + 'static,
    F: CoroutineParamFunction<Marker, T>,
{
    pub fn from_function(
        scope: &Scope,
        owner: Option<Entity>,
        sender: Option<Sender<T>>,
        f: F,
    ) -> Option<Self> {
        let mut meta = CoroMeta {
            owner,
            access: CoroAccess::default(),
        };

        let params = F::Params::init(scope.world_cell_read_only(), &mut meta)?;
        let world_ptr = Box::into_raw(Box::new(std::ptr::null_mut()));
        let ids_ptr = Box::into_raw(Box::new(std::ptr::null()));
        let shared_yield = Box::into_raw(Box::new(None));
        let shared_new_coro = Box::into_raw(Box::default());

        let new_scope = Scope::new(owner, world_ptr, ids_ptr, shared_yield, shared_new_coro);
        let future = f.init(new_scope, params);

        Some(Self {
            future,
            world_ptr,
            shared_yield,
            shared_new_coro,
            ids_ptr,
            meta,
            result_sender: sender,
        })
    }

    pub fn from_world(
        world: &mut World,
        owner: Option<Entity>,
        sender: Option<Sender<T>>,
        f: F,
    ) -> Option<Self> {
        let mut meta = CoroMeta {
            owner,
            access: CoroAccess::default(),
        };

        let params = F::Params::init(world.as_unsafe_world_cell_readonly(), &mut meta)?;
        let world_ptr = Box::into_raw(Box::new(std::ptr::null_mut()));
        let ids_ptr = Box::into_raw(Box::new(std::ptr::null()));
        let shared_yield = Box::into_raw(Box::new(None));
        let shared_new_coro = Box::into_raw(Box::default());

        let new_scope = Scope::new(owner, world_ptr, ids_ptr, shared_yield, shared_new_coro);
        let future = f.init(new_scope, params);

        Some(Self {
            future,
            world_ptr,
            shared_yield,
            shared_new_coro,
            ids_ptr,
            meta,
            result_sender: sender,
        })
    }
}

macro_rules! impl_coro_function {
    ($($param: ident),*) => {
        #[allow(non_snake_case, unused_mut, unused_variables, unused_parens)]
        impl<Func, T, Fut, $($param: CoroParam),*> CoroutineParamFunction<fn($($param,)*) -> Fut, T> for Func
        where
            Func: FnOnce(Scope, $($param),*) -> Fut + Send + 'static,
            Fut: Future<Output = T> + Send + 'static,
            T: Send + Sync + 'static,
        {



            type Future = Fut;
            type Params = ($($param),*);

            fn init(self, scope: Scope, params: Self::Params) -> Self::Future {
                let ($(($param)),*) = params;
                self(scope, $($param),*)
            }
        }
    };
}

all_tuples!(impl_coro_function, 0, 16, P);
