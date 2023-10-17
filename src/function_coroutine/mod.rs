use bevy::ecs::system::CommandQueue;
use bevy::ecs::world::World;

use bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell;
use bevy::utils::all_tuples;
use std::future::Future;

use std::pin::Pin;

use std::task::Context;
use std::task::Poll;

use pin_project::pin_project;

use self::once_channel::OnceSender;
use self::resume::Resume;
use self::scope::ResumeParam;
use self::scope::Scope;

use super::coro_param::CoroParam;
use super::CoroAccess;
use super::CoroMeta;

use super::executor::msg::CoroStatus;
use super::executor::msg::EmitMsg;
use super::executor::msg::NewCoroutine;

use super::id_alloc::Id;
use super::id_alloc::Ids;
use super::Coroutine;

pub mod await_all;
pub mod await_change;
pub mod await_first;
pub mod await_signal;
pub mod await_time;
pub mod handle;
pub mod once_channel;
pub mod resume;
pub mod scope;
pub mod deferred;

pub mod prelude {
    #[doc(hidden)]
    pub use super::scope::Scope;

    #[doc(hidden)]
    pub use super::handle::CoroHandle;
}

#[pin_project]
pub struct FunctionCoroutine<Marker, F, T>
where
    F: CoroutineParamFunction<Marker, T>,
{
    #[pin]
    future: F::Future,
    id: Id,
    resume_param: Resume<ResumeParam>,
    meta: CoroMeta,
    result_sender: Option<OnceSender<T>>,
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
    fn resume(
        self: Pin<&mut Self>,
        world: &mut World,
        ids: &Ids,
        curr_node: usize,
        next_coro_channel: &mut Vec<NewCoroutine>,
        emit_signal: &mut Vec<EmitMsg>,
        commands: &mut CommandQueue,
    ) -> CoroStatus {
        let waker = waker::create();
        // Dummy context
        let mut cx = Context::from_waker(&waker);

        let this = self.project();

        let world = world as *mut _;
        let new_coro_sender = next_coro_channel as *mut _;
        let emit_sender = emit_signal as *mut _;
        let commands = commands as *mut _;
        let ids = ids as *const _;

        // Safety: The only unsafe operations are swapping the resume arguments back and forth
        // All the pointers are valid since we get them from references, and we are never doing
        // the swap while the future is getting polled, only before and after.
        unsafe {
            this.resume_param.set(ResumeParam {
                world,
                ids,
                curr_node,
                yield_sender: None,
                new_coro_sender,
                emit_sender,
                commands,
            });

            let res = this.future.poll(&mut cx);

            match res {
                Poll::Ready(t) => {
                    if let Some(sender) = this.result_sender.take() {
                        sender.send(t);
                    }
                    CoroStatus::Done
                }
                _ => {
                    let yield_ = this
                        .resume_param
                        .get_mut()
                        .yield_sender
                        .take()
                        .expect(ERR_WRONGAWAIT);
                    this.resume_param.set(ResumeParam::new());
                    yield_
                }
            }
        }
    }

    fn is_valid(&self, world: &World) -> bool {
        if let Some(sender) = &self.result_sender {
            if !sender.is_alive() {
                return false;
            }
        }

        return F::Params::is_valid(world.as_unsafe_world_cell_readonly(), &self.meta);
    }

    fn meta(&self) -> &CoroMeta {
        &self.meta
    }
}

mod waker {
    use std::task::{RawWaker, RawWakerVTable, Waker};

    pub fn create() -> Waker {
        // Safety: The waker points to a vtable with functions that do nothing. Doing
        // nothing is memory-safe.
        unsafe { Waker::from_raw(RAW_WAKER) }
    }

    const RAW_WAKER: RawWaker = RawWaker::new(std::ptr::null(), &VTABLE);
    const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, do_nothing, do_nothing, do_nothing);

    unsafe fn clone(_: *const ()) -> RawWaker {
        RAW_WAKER
    }
    unsafe fn do_nothing(_: *const ()) {}
}

impl<Marker: 'static, F, T> FunctionCoroutine<Marker, F, T>
where
    T: Send + Sync + 'static,
    F: CoroutineParamFunction<Marker, T>,
{
    pub(crate) fn new(
        scope: Scope,
        world_cell: UnsafeWorldCell,
        resume_param: Resume<ResumeParam>,
        id: Id,
        result_sender: Option<OnceSender<T>>,
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
            resume_param,
            meta,
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

#[derive(PartialEq, Eq, Clone, Copy)]
enum CoroState {
    Halted,
    Running,
}
