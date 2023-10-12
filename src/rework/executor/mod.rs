use bevy::{prelude::Entity, time::Time, utils::synccell::SyncCell};
use std::{collections::VecDeque, ops::Index};

use bevy::{
    prelude::{Resource, World},
    time::Timer,
    utils::HashMap,
};
use tinyset::{SetU64, SetUsize};

use self::{global_channel::{GlobalSender, GlobalReceiver, global_channel}, msg::{YieldMsg, NewCoroutine, CoroStatus, EmitMsg}};

use super::{
    function_coroutine::{CoroutineParamFunction, FunctionCoroutine, scope::{ResumeParam, Scope}, resume::Resume},
    id_alloc::{Id, Ids}, HeapCoro, Coroutine,
};

pub mod global_channel;
pub mod msg;

#[derive(Resource)]
pub struct Executor {
    ids: Ids,
    coroutines: HashMap<Id, HeapCoro>,
    waiting_on_tick: VecDeque<Id>,
    waiting_on_time: VecDeque<(Id, Timer)>,
    waiting_on_all: HashMap<Id, SetU64>,
    waiting_on_first: HashMap<Id, SetU64>,
    scope_ownership: HashMap<Id, SetU64>,
    is_awaited_by: HashMap<Id, Id>,
    yield_channel: (GlobalSender<YieldMsg>, GlobalReceiver<YieldMsg>),
    new_coro_channel: (GlobalSender<NewCoroutine>, GlobalReceiver<NewCoroutine>),
    signal_channel: (GlobalSender<EmitMsg>, GlobalReceiver<EmitMsg>),
}

impl Default for Executor {
    fn default() -> Self {
        Self {
            ids: Default::default(),
            coroutines: Default::default(),
            waiting_on_tick: Default::default(),
            waiting_on_time: Default::default(),
            waiting_on_all: Default::default(),
            waiting_on_first: Default::default(),
            scope_ownership: Default::default(),
            is_awaited_by: Default::default(),
            yield_channel: global_channel(),
            new_coro_channel: global_channel(),
            signal_channel: global_channel(),
        }
    }
}

// SAFETY: The [`Executor`] can only be accessed througth an exclusive
// reference, therefore it never has to be synced.
unsafe impl Sync for Executor {}

impl Executor {
    pub fn add_coroutine(&mut self, id: Id, coroutine: HeapCoro) {
        let prev = self.coroutines.insert(id, coroutine);
        self.waiting_on_tick.push_back(id);
        debug_assert!(prev.is_none());
    }

    fn cancel(&mut self, coro_id: Id) {
        self.ids.free(coro_id);
        self.coroutines.remove(&coro_id);

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

        if !coro.as_mut().is_valid(world) {
            self.cancel(coro_id);
            return;
        }

        Coroutine::resume(coro.as_mut(), world, &self.ids, coro_node);

        // TODO Signals

        while let Some(NewCoroutine {
            id,
            ran_after,
            coroutine,
            is_owned_by,
            should_start_now,
        }) = unsafe { self.new_coro_channel.1.try_recv_sync() }
        {
            self.coroutines.insert(id, coroutine);

            if let Some(parent) = is_owned_by {
                self.scope_ownership
                    .entry(parent)
                    .or_default()
                    .insert(id.to_bits());
            }

            if should_start_now {
                let next_node = parents.add_child(ran_after, id);
                ready_coro.push_back((id, next_node));
            }
        }

        let YieldMsg { id, node, status } =
            unsafe { self.yield_channel.1.try_recv_sync().unwrap() };

        match status {
            CoroStatus::Done => {
                self.mark_as_done(id, node, ready_coro, parents);
            }
            CoroStatus::Tick => self.waiting_on_tick.push_back(id),
            CoroStatus::Duration(d) => self.waiting_on_time.push_back((id, d)),
            CoroStatus::First(handlers) => {
                self.waiting_on_first.insert(id, handlers.clone());
                for handler in handlers.iter() {
                    self.is_awaited_by.insert(Id::from_bits(handler), id);
                }
            }
            CoroStatus::All(handlers) => {
                self.waiting_on_all.insert(id, handlers.clone());
                for handler in handlers.iter() {
                    self.is_awaited_by.insert(Id::from_bits(handler), id);
                }
            }
            CoroStatus::Cancel => {
                self.cancel(id);
            }
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
        self.coroutines.remove(&coro_id);

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

    pub fn add_function_coroutine<Marker: 'static, T, C>(
        &mut self,
        owner: Option<Entity>,
        world: &mut World,
        coroutine: C,
    ) where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        let resume_param = Resume::new(ResumeParam::new());

        let id = self.ids.allocate_id();

        let new_scope = Scope::new(
            id,
            owner,
            resume_param.get_raw(),
            self.yield_channel.0.clone(),
            self.new_coro_channel.0.clone(),
            self.signal_channel.0.clone(),
        );

        if let Some(c) = FunctionCoroutine::new(
            new_scope,
            world.as_unsafe_world_cell_readonly(),
            resume_param,
            self.yield_channel.0.clone(),
            id,
            None,
            coroutine,
        ) {
            self.add_coroutine(id, SyncCell::new(Box::pin(c)));
        };
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