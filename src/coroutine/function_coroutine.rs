use std::time::Duration;
use std::{cell::Cell, rc::Rc};

use bevy::prelude::Entity;
use bevy::prelude::World;
use bevy::utils::all_tuples;
use bevy::utils::synccell::SyncCell;
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use crate::coroutine::CoroMeta;
use crate::executor::msg_channel::Receiver;
use crate::executor::msg_channel::Sender;

use super::coro_param::CoroParam;
use super::coro_param::ParamContext;
use super::coro_param::WorldWindow;
use super::duration::DurationFuture;
use super::duration::NextTick;
use super::par_and::ParAnd;
use super::par_or::ParOr;
use super::UninitCoroutine;
use super::{waker, Coroutine, CoroutineResult, WaitingReason};

#[pin_project]
pub struct FunctionCoroutine<Marker, F>
where
    F: CoroutineParamFunction<Marker>,
{
    #[pin]
    future: F::Future,
    yield_channel: Receiver<WaitingReason>,
    world_window: Rc<Cell<*mut World>>,
    meta: CoroMeta,
}

unsafe impl<Marker, F> Send for FunctionCoroutine<Marker, F> where F: CoroutineParamFunction<Marker> {}

const ERR_WRONGAWAIT: &str = "A coroutine yielded without notifying the executor
the reason. That is most likely because it awaits a
future which is not part of this library.";

impl<Marker: 'static, F> Coroutine for FunctionCoroutine<Marker, F>
where
    F: CoroutineParamFunction<Marker>,
{
    fn resume(self: Pin<&mut Self>, world: &mut World) -> CoroutineResult<WaitingReason, ()> {
        let waker = waker::create();
        // Dummy context
        let mut cx = Context::from_waker(&waker);

        let this = self.project();
        this.world_window.replace(world as *mut _);
        let res = this.future.poll(&mut cx);
        this.world_window.replace(std::ptr::null_mut());
        match res {
            Poll::Ready(_) => CoroutineResult::Done(()),
            Poll::Pending => {
                CoroutineResult::Yield(this.yield_channel.receive().expect(ERR_WRONGAWAIT))
            }
        }
    }

    fn is_valid(self: Pin<&mut Self>, world: &World) -> bool {
        //TODO validate the window as well
        F::Params::is_valid(self.meta.owner, world)
    }
}

pub trait CoroutineParamFunction<Marker>: Send + 'static {
    type Future: Future<Output = ()> + Send + 'static;
    type Params: CoroParam;

    fn init(self, fib: Fib, params: Self::Params) -> Self::Future;
}

impl<Marker: 'static, F> UninitCoroutine<Marker> for F
where
    F: CoroutineParamFunction<Marker>,
{
    type Coroutine = FunctionCoroutine<Marker, F>;

    fn init(self, owner: Entity, world: &mut World) -> Option<Self::Coroutine> {
        let yield_channel = Receiver::default();
        let world_window = Rc::new(Cell::new(std::ptr::null_mut()));

        let context = ParamContext {
            owner,
            world_window: WorldWindow(Rc::clone(&world_window)),
            yield_sender: yield_channel.sender(),
        };

        let mut meta = CoroMeta::new(owner);

        let params = F::Params::init(context, world, &mut meta)?;
        let fib = Fib {
            world_window: WorldWindow(Rc::clone(&world_window)),
            yield_channel: yield_channel.sender(),
            meta: meta.clone(),
        };

        Some(FunctionCoroutine {
            future: self.init(fib, params),
            yield_channel,
            world_window,
            meta,
        })
    }
}

/// The `Fib` is the first param of a coroutine, all yielding is done througth it.
pub struct Fib {
    pub(crate) world_window: WorldWindow,
    pub(crate) yield_channel: Sender<WaitingReason>,
    pub(crate) meta: CoroMeta,
}

// Safety: No idea....
unsafe impl Send for Fib {}
unsafe impl Sync for Fib {}

impl Fib {
    /// Returns coroutine that resolve the next time the [`Executor`] is ticked (via
    /// [`run`][crate::executor::Executor::run] for instance). It returns the duration
    /// of the last frame (delta time).
    ///
    /// [`Executor`]: crate::executor::Executor
    pub fn next_tick(&self) -> NextTick<'_> {
        NextTick::new(self)
    }

    ///// Returns a coroutine that resolve after a certain [`Duration`]. Note that if the duration
    ///// is smaller than the time between two tick of the [`Executor`] it won't be compensated.
    /////
    ///// [`Executor`]: crate::executor::Executor
    pub fn duration(&self, duration: Duration) -> DurationFuture<'_> {
        DurationFuture::new(self, duration)
    }

    ///// Returns a coroutine that resolve once any of the underlying coroutine finishes. Note that
    ///// once this is done, all the others are dropped. The coroutines are resumed from top to
    ///// bottom, in case multiple of them are ready to make progress at the same time.
    pub fn par_or<C, Marker: 'static>(&self, coro: C) -> ParOr<'_>
    where
        C: UninitCoroutine<Marker>,
        C::Coroutine: 'static,
    {
        // Safety: We are getting polled right now, therefore we have exclusive world access.
        unsafe {
            if let Some(c) = coro.init(self.meta.owner, self.world_window.world_cell().world_mut())
            {
                return ParOr::new(self, vec![SyncCell::new(Box::pin(c))]);
            }
        }
        ParOr::new(self, vec![])
    }

    ///// Returns a coroutine that resolve once all of the underlying coroutine finishes.
    pub fn par_and<C, Marker>(&mut self, coro: C) -> ParAnd<'_>
    where
        C: UninitCoroutine<Marker>,
    {
        // Safety: We are getting polled right now, therefore we have exclusive world access.
        unsafe {
            if let Some(c) = coro.init(self.meta.owner, self.world_window.world_cell().world_mut())
            {
                return ParAnd::new(self, vec![SyncCell::new(Box::pin(c))]);
            }
        }
        ParAnd::new(self, vec![])
    }

    ///// Returns a coroutine that resolve once the underlying coroutine finishes,
    ///// in order to reuse coroutines, because the following won't compile:
    ///// ```compile_fail
    /////# use corentin::prelude::*;
    /////async fn sub_coro(mut fib: Fib) { }
    /////async fn main_coro(mut fib: Fib) {
    /////  sub_coro(fib).await;
    /////  sub_coro(fib).await;
    /////}
    /////```
    ///// But the following will:
    /////```
    /////# use corentin::prelude::*;
    /////async fn sub_coro(mut fib: Fib) { }
    /////async fn main_coro(mut fib: Fib) {
    /////  fib.on(sub_coro).await;
    /////  fib.on(sub_coro).await;
    /////}
    /////```
    //pub fn on<C, Marker>(&mut self, coro: C) -> On<C::Coroutine>
    //where
    //    C: UninitCoroutine<Marker>,
    //{
    //    unsafe {
    //        if let Some(c) = coro.init(self.owner, self.world_window.world_cell().world_mut()) {
    //            return On::new(self, c);
    //        }
    //    }
    //    panic!()
    //}
}

macro_rules! impl_coro_function {
    ($($param: ident),*) => {
        #[allow(non_snake_case, unused_mut, unused_variables, unused_parens)]
        impl<Func, Fut, $($param: CoroParam),*> CoroutineParamFunction<fn(Fib, $($param,)*) -> Fut> for Func
        where
            Func: FnOnce(Fib, $($param),*) -> Fut + Send + 'static,
            Fut: Future<Output = ()> + Send + 'static,
        {

            type Future = Fut;
            type Params = ($($param),*);

            fn init(self, fib: Fib, params: Self::Params) -> Self::Future {
                let ($(($param)),*) = params;
                self(fib, $($param),*)
            }
        }
    };
}

macro_rules! impl_coro_no_fib_function {
    ($($param: ident),*) => {
        #[allow(non_snake_case, unused_mut, unused_variables, unused_parens)]
        impl<Func, Fut, $($param: CoroParam),*> CoroutineParamFunction<fn($($param,)*) -> Fut> for Func
        where
            Func: FnOnce($($param),*) -> Fut + Send + 'static,
            Fut: Future<Output = ()> + Send + 'static,
        {

            type Future = Fut;
            type Params = ($($param),*);

            fn init(self, _fib: Fib, params: Self::Params) -> Self::Future {
                let ($(($param)),*) = params;
                self($($param),*)
            }
        }
    };
}

all_tuples!(impl_coro_function, 0, 16, P);
all_tuples!(impl_coro_no_fib_function, 0, 16, P);
