use bevy::{
    prelude::Entity,
    time::Time,
    utils::{synccell::SyncCell, HashSet},
};
use std::{collections::VecDeque, ops::Index};

use bevy::{
    prelude::{Resource, World},
    time::Timer,
    utils::HashMap,
};
use tinyset::{SetU64, SetUsize};

use crate::{
    function_coroutine::ResumeParam,
    global_channel::{Channel, CommandChannel},
};

use self::msg::{CoroStatus, EmitMsg, NewCoroutine, SignalId, YieldMsg};

use super::{
    function_coroutine::{resume::Resume, scope::Scope, CoroutineParamFunction, FunctionCoroutine},
    id_alloc::{Id, Ids},
    Coroutine, HeapCoro,
};

pub mod msg;

#[derive(Resource, Default)]
pub struct Executor {
    ids: Ids,
    coroutines: HashMap<Id, HeapCoro>,
    waiting_on_tick: VecDeque<Id>,
    waiting_on_time: VecDeque<(Id, Timer)>,
    waiting_on_all: HashMap<Id, SetU64>,
    waiting_on_first: HashMap<Id, SetU64>,
    waiting_on_signal: HashMap<SignalId, SetU64>,
    scope_ownership: HashMap<Id, SetU64>,
    is_awaited_by: HashMap<Id, Id>,
    new_coro_channel: Channel<NewCoroutine>,
    signal_channel: Channel<EmitMsg>,
    commands_channel: CommandChannel,
    yield_channel: Channel<YieldMsg>,
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

        let mut parents = ParentTable::new();
        let mut signals = HashMap::new();

        let mut ready_coro: VecDeque<(Id, usize)> = root_coros
            .into_iter()
            .map(|c_id| (c_id, parents.add_root(c_id)))
            .collect();

        while !ready_coro.is_empty() {
            while let Some((coro_id, node)) = ready_coro.pop_front() {
                if !self.ids.contains(coro_id) {
                    continue;
                }

                let coro = self.coroutines.get_mut(&coro_id).unwrap().get();

                if !coro.is_valid(world) {
                    self.cancel(coro_id);
                    continue;
                }

                Coroutine::resume(
                    coro.as_mut(),
                    world,
                    &self.ids,
                    node,
                    &self.signal_channel,
                    &self.new_coro_channel,
                    &self.commands_channel,
                    &self.yield_channel,
                );
            }
            self.process_channels(&mut ready_coro, &mut parents, &mut signals);
        }

        self.ids.flush();
        self.commands_channel.apply(world);
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
        self.just_done.insert(coro_id.to_bits());

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
        world: &World,
        coroutine: C,
    ) where
        C: CoroutineParamFunction<Marker, T>,
        T: Sync + Send + 'static,
    {
        let resume_param = Resume::new(ResumeParam::new());

        let id = self.ids.allocate_id();

        let new_scope = Scope::new(id, owner, resume_param.get_raw());

        if let Some(c) = FunctionCoroutine::new(
            new_scope,
            world.as_unsafe_world_cell_readonly(),
            resume_param,
            id,
            None,
            coroutine,
        ) {
            self.add_coroutine(id, SyncCell::new(Box::pin(c)));
        };
    }

    fn process_channels(
        &mut self,
        ready_coro: &mut VecDeque<(Id, usize)>,
        parents: &mut ParentTable,
        signal_table: &mut HashMap<SignalId, usize>,
    ) {
        for NewCoroutine {
            id,
            ran_after,
            coroutine,
            is_owned_by,
            should_start_now,
        } in self.new_coro_channel.receive()
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

        let mut just_done: HashMap<u64, usize> = HashMap::new();

        for YieldMsg { id, node, status } in self.yield_channel.receive() {
            match status {
                CoroStatus::Done => {
                    just_done.insert(id.to_bits(), node);
                    self.mark_as_done(id, node, ready_coro, parents);
                }
                CoroStatus::Tick => self.waiting_on_tick.push_back(id),
                CoroStatus::Duration(d) => self.waiting_on_time.push_back((id, d)),
                CoroStatus::First(mut handlers) => {
                    if handlers
                        .iter()
                        .find(|h| {
                            !just_done.contains_key(h) && !self.ids.contains(Id::from_bits(*h))
                        })
                        .is_some()
                    {

                    }
                    if let Some(p_id) = handlers.iter().find(|h| just_done.contains_key(h)) {
                        handlers.remove(p_id);
                        // coro is the "winner", all the others are cancelled
                        for o in handlers {
                            let id = Id::from_bits(o);
                            self.cancel(id);
                        }

                        let node =
                            parents.add_child(*just_done.get(&p_id).unwrap(), Id::from_bits(p_id));
                        ready_coro.push_back((id, node));
                        continue;
                    }

                    self.waiting_on_first.insert(id, handlers.clone());

                    for handler in handlers.iter() {
                        self.is_awaited_by.insert(Id::from_bits(handler), id);
                    }
                }
                CoroStatus::All(handlers) => {
                    let mut waits_on = handlers.clone();

                    for handler in handlers.iter() {
                        // if let Some()
                        self.is_awaited_by.insert(Id::from_bits(handler), id);
                    }

                    self.waiting_on_all.insert(id, waits_on);
                }
                CoroStatus::Cancel => {
                    self.cancel(id);
                }
                CoroStatus::Signal(signal_id) => {
                    if let Some(writer) = signal_table.get(&signal_id) {
                        if !parents.is_parent(*writer, node) {
                            let node = parents.add_child(*writer, id);
                            ready_coro.push_back((id, node));
                            return;
                        }
                    }

                    self.waiting_on_signal
                        .entry(signal_id)
                        .or_default()
                        .insert(id.to_bits());
                }
            };
        }

        for EmitMsg { id, by } in self.signal_channel.receive() {
            signal_table.insert(id, by);
            if let Some(children) = self.waiting_on_signal.remove(&id) {
                for c in children {
                    let id = Id::from_bits(c);
                    let node = parents.add_child(by, id);
                    ready_coro.push_back((id, node));
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
    /// node, then it will create a new node for `child` who inherits all its previous parents (to
    /// preserve history)
    fn add_child(&mut self, parent: usize, child: Id) -> usize {
        let mut parents = SetUsize::default();
        if let Some(node) = self.node_map.get(&child) {
            parents = self.table.get(*node).unwrap().clone();
        }

        parents.extend(self.table.index(parent).clone());
        parents.insert(parent);
        let node = self.table.len();

        self.node_map.insert(child, node);
        self.table.push(parents);

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
}
