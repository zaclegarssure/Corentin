use std::marker::PhantomData;

use bevy::prelude::Component;

/////
//pub trait SignalType<T> {
//    fn read(&self) -> &T;
//
//    fn write(&mut self, t: T);
//}
//
//#[derive(Component)]
//pub struct Signal<S, T>
//where
//    S: SignalType<T> + Send + Sync + 'static,
//    T: Send + Sync + 'static 
//{
//    value: S,
//    _phantom: PhantomData<T>
//}
//
//
