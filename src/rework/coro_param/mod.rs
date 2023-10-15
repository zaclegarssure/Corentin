use bevy::{ecs::world::unsafe_world_cell::UnsafeWorldCell, utils::all_tuples};

use super::CoroMeta;

pub mod component;
pub mod signals;

/// A function taking a scope and 0 or many [`CoroParam`]
/// can be trurned into a [`Coroutine`](super::Coroutine).
pub trait CoroParam: Sized {
    /// Initialize this parameter, and update the metadata.
    /// The world can only be used to read metadata.
    fn init(world: UnsafeWorldCell<'_>, coro_meta: &mut CoroMeta) -> Option<Self>;

    /// Return true iff this parameter is still valid.
    /// The world can only be used to read metadata.
    fn is_valid(world: UnsafeWorldCell<'_>, coro_meta: &CoroMeta) -> bool;
}

macro_rules! impl_coro_param {
    ($($param: ident),*) => {
        #[allow(non_snake_case, unused_parens, unused_variables)]
        impl<$($param: CoroParam),*> CoroParam for ($($param,)*) {
            fn init(world: UnsafeWorldCell<'_>, meta: &mut CoroMeta) -> Option<Self> {
                $(let $param = $param::init(world, meta)?;)*

                Some(($($param,)*))

            }

            fn is_valid(world: UnsafeWorldCell<'_>, coro_meta: &CoroMeta) -> bool {
                true $(&& $param::is_valid(world, coro_meta))*
            }
        }

    };
}

all_tuples!(impl_coro_param, 0, 16, P);
