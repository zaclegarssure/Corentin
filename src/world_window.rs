//use std::ops::{Deref, DerefMut};
//
//use bevy::prelude::Component;
//
//// TODO change detection (isn't this what Mut is already doing in Bevy ? Damn looks like I'm tired...)
//pub struct ComponentGuard<'a, T: Component>(&'a T);
//
//impl<'a, T: Component> Deref for ComponentGuard<'a, T> {
//    type Target = T;
//
//    fn deref(&self) -> &Self::Target {
//        self.0
//    }
//}
//
//pub struct ComponentGuardMut<'a, T: Component>(&'a mut T);
//
//impl<'a, T: Component> Deref for ComponentGuardMut<'a, T> {
//    type Target = T;
//
//    fn deref(&self) -> &Self::Target {
//        self.0
//    }
//}
//
//impl<'a, T: Component> DerefMut for ComponentGuardMut<'a, T> {
//    fn deref_mut(&mut self) -> &mut Self::Target {
//        self.0
//    }
//}
