use bevy::time::Time;
use std::{collections::VecDeque, ops::Index};

use bevy::{
    prelude::{Resource, World},
    time::Timer,
    utils::HashMap,
};
use tinyset::{SetU64, SetUsize};

use super::{
    id_alloc::{Id, Ids},
    Coroutine, CoroutineResult, CoroutineStatus, HeapCoro, NewCoroutine, WaitingReason,
};

#[derive(Resource, Default)]
pub struct Executor {
    ids: Ids,
    coroutines: HashMap<Id, HeapCoro>,
    waiting_on_tick: VecDeque<Id>,
    waiting_on_time: VecDeque<(Id, Timer)>,
    waiting_on_all: HashMap<Id, SetU64>,
    waiting_on_first: HashMap<Id, SetU64>,
    scope_ownership: HashMap<Id, SetU64>,
    is_awaited_by: HashMap<Id, Id>,
}

impl Executor {
    pub fn add_coroutine(&mut self, coroutine: HeapCoro) {
        let id = self.ids.allocate_id();
        let prev = self.coroutines.insert(id, coroutine);
        self.waiting_on_tick.push_back(id);
        debug_assert!(prev.is_none());
    }

    fn cancel(&mut self, coro_id: Id) {
        self.ids.free(coro_id);
        if let Some(mut coro) = self.coroutines.remove(&coro_id) {
            coro.get().cleanup();
        }

        if let Some(owned) = self.scope_ownership.remove(&coro_id) {
            for c in owned {
                self.cancel(Id::from_bits(c))
            }
        }

        if let Some(parent) = self.is_awaited_by.remove(&coro_id) {
            self.cancel(parent);
        }

        if let Some(others) = self.waiting_on_first.remove(&coro_id) {
            for o in others {
                self.cancel(Id::from_bits(o));
            }
        }

        if let Some(others) = self.waiting_on_all.remove(&coro_id) {
            for o in others {
                self.cancel(Id::from_bits(o));
            }
        }
    }

    pub fn tick_until_empty(&mut self, world: &mut World) {
        while !self.coroutines.is_empty() {
            self.tick(world);
        }
    }

    pub fn tick(&mut self, world: &mut World) {
        let mut root_coros = VecDeque::<Id>::new();

        root_coros.append(&mut self.waiting_on_tick);

        let delta_time = world.resource::<Time>().delta();

        // Tick all coroutines waiting on duration
        for (_, timer) in &mut self.waiting_on_time {
            timer.tick(delta_time);
        }
        self.waiting_on_time.retain(|(coro, timer)| {
            if timer.just_finished() {
                root_coros.push_back(*coro);
                false
            } else {
                true
            }
        });

        let mut parents = ParentTable::default();

        let mut ready_coro: VecDeque<(Id, usize)> = root_coros
            .into_iter()
            .map(|c_id| (c_id, parents.add_root(c_id)))
            .collect();

        while let Some((coro_id, node)) = ready_coro.pop_front() {
            self.resume(coro_id, node, &mut ready_coro, &mut parents, world);
        }

        self.ids.flush();
    }

    /// Run a specific coroutine and it's children.
    fn resume(
        &mut self,
        coro_id: Id,
        coro_node: usize,
        ready_coro: &mut VecDeque<(Id, usize)>,
        parents: &mut ParentTable,
        world: &mut World,
    ) {
        if !self.ids.contains(coro_id) {
            return;
        }

        let coro = self.coroutines.get_mut(&coro_id).unwrap().get();

        // Safety: The world pointer is valid (and exclusive) and we don't run anything
        // concurrently on the world right now
        if !coro.as_mut().is_valid(world) {
            self.cancel(coro_id);
            return;
        }

        let world = world as *mut _;
        let CoroutineResult { result, new_coro } =
            // Safety: same as the previous one
            unsafe { Coroutine::resume_unsafe(coro.as_mut(), world, &self.ids) };

        // TODO Signals

        for NewCoroutine {
            id,
            coroutine,
            is_owned_by_scope,
            should_start_now,
        } in new_coro
        {
            self.coroutines.insert(id, coroutine);

            if is_owned_by_scope {
                self.scope_ownership
                    .entry(coro_id)
                    .or_default()
                    .insert(id.to_bits());
            }

            if should_start_now {
                let next_node = parents.add_child(coro_node, id);
                ready_coro.push_back((id, next_node));
            }
        }

        match result {
            CoroutineStatus::Done => {
                self.mark_as_done(coro_id, coro_node, ready_coro, parents);
            }
            CoroutineStatus::Yield(yield_msg) => match yield_msg {
                WaitingReason::Tick => self.waiting_on_tick.push_back(coro_id),
                WaitingReason::Duration(d) => self.waiting_on_time.push_back((coro_id, d)),
                WaitingReason::First(handlers) => {
                    self.waiting_on_first.insert(coro_id, handlers.clone());
                    for handler in handlers.iter() {
                        self.is_awaited_by.insert(Id::from_bits(handler), coro_id);
                    }
                }
                WaitingReason::All(handlers) => {
                    self.waiting_on_all.insert(coro_id, handlers.clone());
                    for handler in handlers.iter() {
                        self.is_awaited_by.insert(Id::from_bits(handler), coro_id);
                    }
                }
                WaitingReason::Cancel => {
                    self.cancel(coro_id);
                }
            },
        };
    }

    /// Mark a coroutine as done, and properly handles cleanup.
    fn mark_as_done(
        &mut self,
        coro_id: Id,
        coro_node: usize,
        ready_coro: &mut VecDeque<(Id, usize)>,
        parents: &mut ParentTable,
    ) {
        if let Some(mut coro) = self.coroutines.remove(&coro_id) {
            coro.get().cleanup();
        }

        if let Some(owned) = self.scope_ownership.remove(&coro_id) {
            for c in owned {
                self.cancel(Id::from_bits(c))
            }
        }

        if let Some(parent) = self.is_awaited_by.remove(&coro_id) {
            if let Some(mut others) = self.waiting_on_first.remove(&parent) {
                others.remove(coro_id.to_bits());
                // coro is the "winner", all the others are cancelled
                for o in others {
                    let id = Id::from_bits(o);
                    self.is_awaited_by.remove(&id);
                    self.cancel(id);
                }

                let node = parents.add_child(coro_node, parent);
                ready_coro.push_back((parent, node));
            }

            if let Some(others) = self.waiting_on_all.get_mut(&parent) {
                others.remove(coro_id.to_bits());

                let node = parents.add_child(coro_node, parent);

                if others.is_empty() {
                    ready_coro.push_back((parent, node));
                    self.waiting_on_all.remove(&parent);
                }
            }
        }
    }
}

/// Keep track of who ran after who, to make sure coroutines can react to even they could have
/// seen, and do not react to events they could not see.
#[derive(Default)]
struct ParentTable {
    table: Vec<SetUsize>,
    node_map: HashMap<Id, usize>,
}

impl ParentTable {
    fn new() -> Self {
        Self::default()
    }

    /// Add a child node for coroutine `child`, to the `parent` node. If `child` already has a
    /// node, then this function behaves a bit like [`add_parent`], but it will create a new node
    /// for `child` who inherits all its previous parents (to preserve history)
    fn add_child(&mut self, parent: usize, child: Id) -> usize {
        let mut parents = SetUsize::default();
        if let Some(node) = self.node_map.get(&child) {
            parents = self.table.get(*node).unwrap().clone();
        }

        parents.extend(self.table.index(parent).clone());
        parents.insert(parent);
        let node = self.table.len() - 1;
        self.node_map.insert(child, node);
        node
    }

    fn add_root(&mut self, c: Id) -> usize {
        self.table.push(SetUsize::new());
        let node = self.table.len() - 1;
        self.node_map.insert(c, node);
        node
    }

    /// Return true if (and only if) `parent` is a parent of `child`.
    /// It is useful to know if a write performed by coroutine A can
    /// be observed by coroutine B, which is the case if this returns
    /// false.
    fn is_parent(&self, parent: usize, child: usize) -> bool {
        self.table.get(child).unwrap().contains(parent)
    }

    /// Add a `parent` to an existing `node`.
    fn add_parent(&mut self, parent: usize, node: usize) {
        let parent_parents = self.table.get(parent).unwrap().clone();
        let current = self.table.get_mut(node).unwrap();
        current.insert(parent);
        current.extend(parent_parents);
    }
}
