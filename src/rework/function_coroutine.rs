use crate::rework::NewCoroutine;
use bevy::ecs::world::World;

use bevy::prelude::Entity;
use bevy::utils::all_tuples;
use std::future::Future;

use std::pin::Pin;
use std::ptr::null;
use std::ptr::null_mut;
use std::task::Context;
use std::task::Poll;

use pin_project::pin_project;

use super::coro_param::CoroParam;
use super::CoroAccess;
use super::CoroMeta;

use super::id_alloc::Ids;
use super::one_shot::Sender;
use super::resume::Resume;
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
    world_ptr: Resume<*mut World>,
    shared_yield: Resume<Option<WaitingReason>>,
    shared_new_coro: Resume<Vec<NewCoroutine>>,
    ids_ptr: Resume<*const Ids>,
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
    fn resume(self: Pin<&mut Self>, world: &mut World, ids: &Ids) -> CoroutineResult {
        let waker = waker::create();
        // Dummy context
        let mut cx = Context::from_waker(&waker);

        let this = self.project();

        let world = world as *mut _;
        let ids = ids as *const _;

        // Safety: The only unsafe operations are swapping the resume arguments back and forth
        // All the pointers are valid since we get them from references, and we are never doing
        // the swap while the future is getting polled, only before and after.
        unsafe {
            this.world_ptr.set(world);
            this.ids_ptr.set(ids);
            let res = this.future.poll(&mut cx);
            this.world_ptr.set(null_mut());
            this.ids_ptr.set(std::ptr::null());

            let mut result = CoroutineResult {
                result: CoroutineStatus::Done,
                new_coro: std::mem::take(this.shared_new_coro.get_mut()),
            };

            match res {
                Poll::Ready(t) => {
                    if let Some(sender) = this.result_sender.take() {
                        sender.send_sync(t);
                    }

                    result
                }
                Poll::Pending => {
                    result.result = CoroutineStatus::Yield(
                        this.shared_yield.get_mut().take().expect(ERR_WRONGAWAIT),
                    );
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
        owner: Option<Entity>,
        sender: Option<Sender<T>>,
        f: F,
    ) -> Option<Self> {
        let mut meta = CoroMeta {
            owner,
            access: CoroAccess::default(),
        };

        let params = F::Params::init(scope.world_cell_read_only(), &mut meta)?;
        let world_ptr = Resume::new(null_mut());
        let ids_ptr = Resume::new(null());
        let shared_yield = Resume::new(None);
        let shared_new_coro = Resume::new(Vec::new());

        let new_scope = Scope::new(
            owner,
            world_ptr.get_raw(),
            ids_ptr.get_raw(),
            shared_yield.get_raw(),
            shared_new_coro.get_raw(),
        );
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
        let world_ptr = Resume::new(null_mut());
        let ids_ptr = Resume::new(null());
        let shared_yield = Resume::new(None);
        let shared_new_coro = Resume::new(Vec::new());

        let new_scope = Scope::new(
            owner,
            world_ptr.get_raw(),
            ids_ptr.get_raw(),
            shared_yield.get_raw(),
            shared_new_coro.get_raw(),
        );
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
