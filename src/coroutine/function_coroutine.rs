use std::time::Duration;

use bevy::prelude::Entity;
use bevy::prelude::World;
use bevy::utils::all_tuples;
use pin_project::pin_project;
use std::future::Future;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use super::coro_param::ParamContext;
use super::coro_param::WorldWindow;
use super::coro_param::{CoroParam, YieldChannel};
use super::duration::DurationFuture;
use super::duration::NextTick;
use super::par_and::ParAnd;
use super::par_or::ParOr;
use super::CoroAccess;
use super::UninitCoroutine;
use super::{waker, Coroutine, CoroutineResult, WaitingReason};

#[pin_project]
pub struct FunctionCoroutine<Marker, F>
where
    F: CoroutineParamFunction<Marker>,
{
    #[pin]
    future: F::Future,
    yield_channel: YieldChannel,
    world_window: WorldWindow,
    owner: Entity,
    access: CoroAccess,
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
        let res = this.world_window.scope(world, || this.future.poll(&mut cx));

        match res {
            Poll::Ready(_) => CoroutineResult::Done(()),
            Poll::Pending => {
                CoroutineResult::Yield(this.yield_channel.receive().expect(ERR_WRONGAWAIT))
            }
        }
    }

    fn is_valid(self: Pin<&mut Self>, world: &World) -> bool {
        F::Params::is_valid(self.owner, world)
    }

    fn access(&self) -> &CoroAccess {
        &self.access
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
        let yield_channel = YieldChannel::new();
        let world_window = WorldWindow::closed_window();

        let context = ParamContext { owner };

        let mut access = CoroAccess::default();

        let params = F::Params::init(context, world, &mut access)?;
        let fib = Fib {
            owner,
            world_window: world_window.clone(),
            yield_channel: yield_channel.clone(),
        };

        Some(FunctionCoroutine {
            future: self.init(fib, params),
            yield_channel,
            world_window,
            owner,
            access,
        })
    }
}

/// The fib is a parameter througth which a coroutine can wait on various elementary construct.
/// such as waiting until the next tick, waiting on multiple sub-coroutines.
pub struct Fib {
    pub(crate) owner: Entity,
    pub(crate) world_window: WorldWindow,
    pub(crate) yield_channel: YieldChannel,
}

impl Fib {
    /// Returns coroutine that resolve the next time the [`Executor`] is ticked (via
    /// [`run`][crate::executor::Executor::run] for instance). It returns the duration
    /// of the last frame (delta time).
    ///
    /// [`Executor`]: crate::executor::Executor
    pub fn next_tick(&mut self) -> NextTick<'_> {
        NextTick::new(self)
    }

    ///// Returns a coroutine that resolve after a certain [`Duration`]. Note that if the duration
    ///// is smaller than the time between two tick of the [`Executor`] it won't be compensated.
    /////
    ///// [`Executor`]: crate::executor::Executor
    pub fn duration(&mut self, duration: Duration) -> DurationFuture<'_> {
        DurationFuture::new(self, duration)
    }

    ///// Returns a coroutine that resolve once any of the underlying coroutine finishes. Note that
    ///// once this is done, all the others are dropped. The coroutines are resumed from top to
    ///// bottom, in case multiple of them are ready to make progress at the same time.
    pub fn par_or<C, Marker>(&mut self, coro: C) -> ParOr<'_>
    where
        C: UninitCoroutine<Marker>,
    {
        ParOr::new(self).with(coro)
    }

    ///// Returns a coroutine that resolve once all of the underlying coroutine finishes.
    pub fn par_and<C, Marker>(&mut self, coro: C) -> ParAnd<'_>
    where
        C: UninitCoroutine<Marker>,
    {
        ParAnd::new(self).with(coro)
    }
}

macro_rules! impl_coro_function {
    ($($param: ident),*) => {
        #[allow(non_snake_case, unused_mut, unused_variables, unused_parens)]
        impl<Func, Fut, $($param: CoroParam),*> CoroutineParamFunction<fn($($param,)*) -> Fut> for Func
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

all_tuples!(impl_coro_function, 0, 16, P);
