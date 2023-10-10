use bevy::{ecs::world::unsafe_world_cell::UnsafeWorldCell, utils::all_tuples};

use super::CoroMeta;

/// A function taking a scope and 0 or many [`CoroParam`]
/// can be trurned into a [`Coroutine`](super::Coroutine).
pub trait CoroParam: Sized {
    /// Initialize this parameter, and update the metadata.
    /// The world can only be used to read metadata.
    fn init(world: UnsafeWorldCell<'_>, coro_meta: &mut CoroMeta) -> Option<Self>;
}

macro_rules! impl_coro_param {
    ($($param: ident),*) => {
        #[allow(non_snake_case, unused_parens, unused_variables)]
        impl<$($param: CoroParam),*> CoroParam for ($($param,)*) {
            fn init(world: UnsafeWorldCell<'_>, meta: &mut CoroMeta) -> Option<Self> {
                $(let $param = $param::init(world, meta)?;)*

                Some(($($param,)*))

            }
        }

    };
}

all_tuples!(impl_coro_param, 0, 16, P);
