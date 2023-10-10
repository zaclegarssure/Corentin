use bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell;

use super::CoroMeta;

/// A function taking a scope and 0 or many [`CoroParam`]
/// can be trurned into a [`Coroutine`](super::Coroutine).
pub trait CoroParam: Sized {
    /// Initialize this parameter, and update the metadata.
    /// The world can only be used to read metadata.
    fn init(world: UnsafeWorldCell<'_>, coro_meta: &mut CoroMeta) -> Option<Self>;
}
