use bevy::{ecs::component::ComponentId, prelude::Entity, utils::HashMap};

pub(crate) struct WriteTable<T> {
    table: HashMap<(Entity, ComponentId), T>,
}

impl<T> WriteTable<T> {
    pub(crate) fn new() -> Self {
        WriteTable {
            table: HashMap::new(),
        }
    }

    pub(crate) fn conflicts<'a, I>(&'a self, vals: I) -> impl Iterator<Item = &'a T>
    where
        I: IntoIterator<Item = (Entity, ComponentId)>,
    {
        vals
            .into_iter()
            .filter_map(|k| self.table.get(&k))
    }

    pub(crate) fn insert(&mut self, key: (Entity, ComponentId), value: T) -> Option<T> {
        self.table.insert(key, value)
    }
}
