use std::marker::PhantomData;

use bevy::prelude::{Component, Resource};

/////
pub trait SignalType<T: Copy> {
    fn read(&self) -> T;

    fn write(&mut self, t: T);
}

#[derive(Component, Resource)]
pub struct Signal<S, T>
where
    S: SignalType<T> + Send + Sync + 'static,
    T: Copy + Send + Sync + 'static,
{
    pub value: S,
    _phantom: PhantomData<T>,
}
