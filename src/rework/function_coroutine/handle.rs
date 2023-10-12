use bevy::utils::all_tuples;
use oneshot::TryRecvError;
use tinyset::SetU64;

use crate::rework::id_alloc::Id;

use super::once_channel::Receiver;


/// Value representing an ongoing coroutine. Can be used to await it's result, or cancel the
/// underlying coroutine by dropping it.
pub enum CoroHandle<T> {
    Waiting { id: Id, receiver: Receiver<T> },
    Done(T),
    Canceled,
    Finish,
}

/// Trait so that we can have function generic over a tuple of handles, like await all.
pub trait HandleTuple {
    type Output;

    /// Update the status of each handles,
    fn update_status(&mut self) -> Status;

    // Try to fetch the result from the coroutines in this tuple. If any of the handle is not in
    // the [`CoroHandle::Done`] state, this returns [`None`].
    fn try_fetch(&mut self) -> Option<Self::Output>;
}

pub enum Status {
    Done,
    StillWaiting(SetU64),
    Canceled,
    Consumed,
}

impl Status {
    fn combine(self, f: impl FnOnce() -> Status) -> Status {
        match self {
            Status::Done => f(),
            Status::StillWaiting(mut w) => match f() {
                Status::Done => Status::StillWaiting(w),
                Status::StillWaiting(w2) => {
                    w.extend(w2);
                    Status::StillWaiting(w)
                }
                status => status,
            },
            _ => self,
        }
    }
}

impl<T> HandleTuple for CoroHandle<T> {
    type Output = T;

    fn update_status(&mut self) -> Status {
        match self {
            CoroHandle::Waiting { id, receiver } => match receiver.try_recv() {
                Ok(val) => {
                    *self = CoroHandle::Done(val);
                    Status::Done
                }
                Err(TryRecvError::Empty) => {
                    let mut set = SetU64::new();
                    set.insert(id.to_bits());
                    Status::StillWaiting(set)
                }
                Err(TryRecvError::Disconnected) => {
                    *self = CoroHandle::Canceled;
                    Status::Canceled
                }
            },
            CoroHandle::Done(_) => Status::Done,
            CoroHandle::Canceled => Status::Canceled,
            CoroHandle::Finish => Status::Consumed,
        }
    }

    fn try_fetch(&mut self) -> Option<Self::Output> {
        match self {
            CoroHandle::Waiting { id: _, receiver } => match receiver.try_recv() {
                Ok(val) => {
                    *self = CoroHandle::Finish;
                    Some(val)
                }
                _ => None,
            },
            CoroHandle::Done(_) => match std::mem::replace(self, CoroHandle::Finish) {
                CoroHandle::Done(v) => Some(v),
                _ => unreachable!(),
            },
            _ => None,
        }
    }
}

macro_rules! impl_handler_tuple {
    ($first: ident, $($param: ident),*) => {
        #[allow(non_snake_case)]
        impl<$first: HandleTuple, $($param: HandleTuple),*> HandleTuple for ($first, $($param,)*) {
            type Output = ($first::Output, $($param::Output,)*);


            /// Update the status of each handles,
            fn update_status(&mut self) -> Status {
                let (first, $($param,)*) = self;
                first.update_status()$(.combine(|| $param.update_status()))*
            }

            // Try to fetch the result from the coroutines in this tuple. If any of the handle is not in
            // the [`CoroHandle::Done`] state, this returns [`None`].
            fn try_fetch(&mut self) -> Option<Self::Output> {
                let (first, $($param,)*) = self;
                Some((first.try_fetch()?, $($param.try_fetch()?,)*))

            }
        }
    };
}

all_tuples!(impl_handler_tuple, 2, 16, H);
