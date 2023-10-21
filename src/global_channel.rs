use std::cell::UnsafeCell;

use bevy::{
    ecs::{
        entity::Entities,
        system::{Command, CommandQueue},
    },
    prelude::{Commands, World},
};
use thread_local::ThreadLocal;

pub struct Channel<T: Send> {
    chan: ThreadLocal<UnsafeCell<Vec<T>>>,
}

impl<T: Send> Default for Channel<T> {
    fn default() -> Self {
        Self {
            chan: Default::default(),
        }
    }
}

impl<T: Send> Channel<T> {
    pub fn send(&self, value: T) {
        let cell = self.chan.get_or_default();
        // Safety: We could use refcell, to be safe actually
        unsafe { cell.get().as_mut().unwrap() }.push(value);
    }

    pub fn receive(&mut self) -> impl Iterator<Item = T> + '_ {
        self.chan.iter_mut().flat_map(|q| q.get_mut().drain(..))
    }
}

#[derive(Default)]
pub struct CommandChannel {
    storage: ThreadLocal<UnsafeCell<CommandQueue>>,
}

impl CommandChannel {
    pub fn add(&self, _c: impl Command) {}

    pub fn commands<'a>(&'a self, entities: &'a Entities) -> Commands<'_, '_> {
        let queue = unsafe { self.storage.get_or_default().get().as_mut().unwrap() };

        Commands::new_from_entities(queue, entities)
    }

    pub fn apply(&mut self, world: &mut World) {
        for queue in &mut self.storage {
            queue.get_mut().apply(world);
        }
    }
}
