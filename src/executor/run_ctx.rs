use bevy::{ecs::component::ComponentId, prelude::Entity, utils::HashMap};
use tinyset::SetUsize;

use super::CoroId;

/// Used to store all the writes currently done, and who did it
pub(crate) struct WriteTable<T> {
    pub(crate) table: HashMap<(Entity, ComponentId), T>,
}

impl<T> WriteTable<T> {
    pub(crate) fn new() -> Self {
        WriteTable {
            table: HashMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, location: (Entity, ComponentId), writer: T) -> Option<T> {
        self.table.insert(location, writer)
    }
}

pub(crate) struct ParentTable {
    pub(crate) table: Vec<SetUsize>,
}

impl ParentTable {
    fn new() -> Self {
        ParentTable { table: Vec::new() }
    }

    pub(crate) fn add_child(&mut self, parent: usize) -> usize {
        let mut parents = self.table.get(parent).unwrap().clone();
        parents.insert(parent);
        self.table.push(parents);
        self.table.len() - 1
    }

    pub(crate) fn add_root(&mut self) -> usize {
        self.table.push(SetUsize::new());
        self.table.len() - 1
    }

    /// Return true if (and only if) `parent` is a parent of `child`.
    /// It is useful to know if a write performed by coroutine A can
    /// be observed by coroutine B, which is the case if this returns
    /// false.
    pub(crate) fn is_parent(&self, parent: usize, child: usize) -> bool {
        self.table.get(child).unwrap().contains(parent)
    }

    pub(crate) fn add_parent(&mut self, parent: usize, node: usize) {
        let parent_parents = self.table.get(parent).unwrap().clone();
        let current = self.table.get_mut(node).unwrap();
        current.insert(parent);
        current.extend(parent_parents);
    }
}

pub(crate) struct RunContext {
    pub(crate) write_table: WriteTable<usize>,
    pub(crate) parent_table: ParentTable,
    pub(crate) current_node_map: HashMap<CoroId, usize>,
}

impl RunContext {
    pub(crate) fn new() -> Self {
        RunContext {
            write_table: WriteTable::new(),
            parent_table: ParentTable::new(),
            current_node_map: HashMap::new(),
        }
    }

    pub(crate) fn can_execute_now(
        &self,
        node: usize,
        react_to: &(Entity, ComponentId),
    ) -> Option<usize> {
        match self.write_table.table.get(react_to) {
            Some(writer) if !self.parent_table.is_parent(*writer, node) => Some(*writer),
            _ => None,
        }
    }
}
