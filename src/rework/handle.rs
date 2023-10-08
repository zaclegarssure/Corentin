use std::marker::PhantomData;

use bevy::{
    prelude::{Component, Entity},
    utils::all_tuples,
};
use tinyset::SetU64;

use super::scope::Scope;

/// Value representing an ongoing coroutine. Can be used to await it's result, or cancel the
/// underlying coroutine by dropping it.
pub struct CoroHandle<T> {
    pub(crate) id: Entity,
    _phantom: PhantomData<T>,
}

impl<T: Sync + Send + 'static> CoroHandle<T> {
    /// Build a new Handler of the [`Coroutine`] with id `id`.
    fn new(id: Entity) -> Self {
        Self {
            id,
            _phantom: PhantomData,
        }
    }

    /// Fetch the result from the coroutine [`Entity`].
    ///
    /// # Safety
    /// The caller must make sure that it has exclusive access to the coroutine who's result is
    /// being taken.
    pub unsafe fn fetch(&self, scope: &Scope) -> T {
        scope
            .world_cell()
            .get_entity(self.id)
            .unwrap()
            .get_mut::<HandledResult<T>>()
            .unwrap()
            .0
            .take()
            .unwrap()
    }

    /// Try to fetch the result from the coroutine [`Entity`], if it is available.
    ///
    /// # Safety
    /// The caller must make sure that it has exclusive access to the coroutine who's result is
    /// being taken.
    pub unsafe fn try_fetch(&self, scope: &Scope) -> Option<T> {
        let res = scope
            .world_cell()
            .get_entity(self.id)?
            .get_mut::<HandledResult<T>>()?
            .0
            .take()?;
        Some(res)
    }
}

/// Trait so that we can have function generic over a tuple of handlers.
pub trait HandlerTuple: 'static {
    type Output;

    /// Fetch the result from the coroutines in this tuple.
    ///
    /// # Safety
    /// The caller must make sure that it has exclusive access to the coroutines who's result is
    /// being taken.
    unsafe fn fetch(&self, scope: &mut Scope) -> Self::Output;

    /// Returns the set of all [`Coroutine`]s ids within that tuple.
    fn to_set(&self) -> SetU64;
}

impl<T: Send + Sync + 'static> HandlerTuple for CoroHandle<T> {
    type Output = T;

    unsafe fn fetch(&self, scope: &mut Scope) -> Self::Output {
        self.fetch(scope)
    }

    fn to_set(&self) -> SetU64 {
        let mut result = SetU64::new();
        result.insert(self.id.to_bits());
        result
    }
}

/// Each handle coroutines put their result in their `HandledResult` component.
#[derive(Component)]
pub(crate) struct HandledResult<T: Send + Sync + 'static>(pub(crate) Option<T>);

macro_rules! impl_handler_tuple {
    ($first: ident, $($param: ident),*) => {
        #[allow(non_snake_case)]
        impl<$first: HandlerTuple, $($param: HandlerTuple),*> HandlerTuple for ($first, $($param,)*) {
            type Output = ($first::Output, $($param::Output,)*);

            unsafe fn fetch(&self, scope: &mut Scope) -> Self::Output {
                let (first, $($param,)*) = self;
                (first.fetch(scope), $($param.fetch(scope)),*)
            }

            fn to_set(&self) -> SetU64 {
                let (first, $($param,)*) = self;
                let mut result = first.to_set();
                $(result.extend($param.to_set());)*
                result
            }
        }
    };
}

all_tuples!(impl_handler_tuple, 2, 16, H);
