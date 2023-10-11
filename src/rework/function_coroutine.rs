use bevy::ecs::world::World;

use bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell;
use bevy::utils::all_tuples;
use std::future::Future;

use std::pin::Pin;
use std::ptr::null_mut;
use std::task::Context;
use std::task::Poll;

use pin_project::pin_project;

use super::coro_param::CoroParam;
use super::CoroAccess;
use super::CoroMeta;
use super::CoroStatus;
use super::YieldMsg;

use super::global_channel::GlobalSender;
use super::id_alloc::Id;
use super::id_alloc::Ids;
use super::one_shot::Sender;
use super::resume::Resume;
use super::scope::Scope;
use super::waker;
use super::Coroutine;

#[pin_project]
pub struct FunctionCoroutine<Marker, F, T>
where
    F: CoroutineParamFunction<Marker, T>,
{
    #[pin]
    future: F::Future,
    id: Id,
    world_param: Resume<*mut World>,
    ids_param: Resume<*const Ids>,
    curr_node_param: Resume<usize>,
    is_paralel_param: Resume<bool>,
    yield_sender: GlobalSender<YieldMsg>,
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
    fn resume(self: Pin<&mut Self>, world: &mut World, ids: &Ids, curr_node: usize) {
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
            this.world_param.set(world);
            this.ids_param.set(ids);
            this.curr_node_param.set(curr_node);
            this.is_paralel_param.set(false);

            let res = this.future.poll(&mut cx);

            this.world_param.set(null_mut());
            this.ids_param.set(std::ptr::null());

            if let Poll::Ready(t) = res {
                if let Some(sender) = this.result_sender.take() {
                    sender.send_sync(t);
                }
                this.yield_sender.send_sync(YieldMsg {
                    id: *this.id,
                    node: curr_node,
                    status: CoroStatus::Done,
                });
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
    pub(crate) fn new(
        scope: Scope,
        world_cell: UnsafeWorldCell,
        world_param: Resume<*mut World>,
        ids_param: Resume<*const Ids>,
        curr_node_param: Resume<usize>,
        is_paralel_param: Resume<bool>,
        yield_sender: GlobalSender<YieldMsg>,
        id: Id,
        result_sender: Option<Sender<T>>,
        f: F,
    ) -> Option<Self> {
        let mut meta = CoroMeta {
            owner: scope.owner(),
            access: CoroAccess::default(),
        };

        let params = F::Params::init(world_cell, &mut meta)?;
        let future = f.init(scope, params);

        Some(Self {
            future,
            world_param,
            ids_param,
            curr_node_param,
            is_paralel_param,
            meta,
            yield_sender,
            id,
            result_sender,
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
