use bevy::time::Time;
use std::collections::VecDeque;

use bevy::{
    prelude::{Entity, Resource, World},
    time::Timer,
    utils::{HashMap, HashSet},
};
use tinyset::{SetU64, SetUsize};

use super::{
    CoroObject, CoroType, Coroutine, CoroutineResult, CoroutineStatus, NewCoroutine, WaitingReason,
};

#[derive(Resource, Default)]
struct Executor {
    coroutines: HashMap<Entity, CoroObject>,
    waiting_on_tick: VecDeque<Entity>,
    waiting_on_time: VecDeque<(Entity, Timer)>,
    waiting_first: HashMap<Entity, SetU64>,
    waiting_all: HashMap<Entity, SetU64>,
    scope_ownership: HashMap<Entity, SetU64>,
    is_awaited_by: HashMap<Entity, Option<Entity>>,
    done_but_not_awaited: HashSet<Entity>,
}

impl Executor {
    fn add(&mut self, coroutine: CoroObject, id: Entity) {
        self.coroutines.insert(id, coroutine).unwrap();
    }

    fn cancel(&mut self, coroutine: Entity, world: &mut World) {
        self.coroutines.remove(&coroutine);
        self.done_but_not_awaited.remove(&coroutine);
        world.despawn(coroutine);

        if let Some(Some(parent)) = self.is_awaited_by.remove(&coroutine) {
            self.cancel(parent, world);
        }
        if let Some(children) = self.scope_ownership.remove(&coroutine) {
            for c in children {
                self.cancel(Entity::from_bits(c), world);
            }
        }

        if let Some(others) = self.waiting_first.remove(&coroutine) {
            for o in others {
                self.cancel(Entity::from_bits(o), world);
            }
        }
        if let Some(others) = self.waiting_all.remove(&coroutine) {
            for o in others {
                self.cancel(Entity::from_bits(o), world);
            }
        }
    }

    fn tick(&mut self, world: &mut World) {
        let mut root_coros = VecDeque::<Entity>::new();

        root_coros.append(&mut self.waiting_on_tick);

        let delta_time = world.resource::<Time>().delta();
        // Tick all coroutines waiting on duration
        for (_, t) in self.waiting_on_time.iter_mut() {
            t.tick(delta_time);
        }
        self.waiting_on_time.retain(|(coro, t)| {
            if t.just_finished() {
                root_coros.push_back(*coro);
            }
            !t.just_finished()
        });

        let mut run_cx = RunContext::default();
        let mut ready_coro: VecDeque<(Entity, usize)> = root_coros
            .into_iter()
            .map(|c_id| (c_id, run_cx.parent_table.add_root(c_id)))
            .collect();

        while let Some((c_id, node)) = ready_coro.pop_front() {
            self.resume(c_id, node, &mut run_cx, &mut ready_coro, world);
        }
    }

    /// Run a specific coroutine and it's children.
    fn resume(
        &mut self,
        coro_id: Entity,
        this_node: usize,
        run_cx: &mut RunContext,
        ready_coro: &mut VecDeque<(Entity, usize)>,
        world: &mut World,
    ) {
        if !self.coroutines.contains_key(&coro_id) {
            return;
        }

        let coro = self.coroutines.get_mut(&coro_id).unwrap().get();
        if !coro.as_mut().is_valid(world) {
            self.cancel(coro_id, world);
            return;
        }

        let CoroutineResult { result, new_coro } = Coroutine::resume(coro.as_mut(), world);

        // TODO Signals

        for NewCoroutine {
            id,
            coroutine,
            coro_type,
            should_start_now,
        } in new_coro
        {
            self.coroutines.insert(id, coroutine);
            match coro_type {
                CoroType::Local => {
                    self.scope_ownership
                        .entry(coro_id)
                        .or_default()
                        .insert(id.to_bits());
                }
                CoroType::Handled => {
                    self.is_awaited_by.insert(id, None);
                }
                CoroType::Background => {}
            };

            if should_start_now {
                let next_node = run_cx.parent_table.add_child(this_node, id);
                ready_coro.push_back((id, next_node));
            }
        }

        match result {
            CoroutineStatus::Done => {
                if let Some(next) = self.mark_as_done(coro_id, this_node, world, run_cx) {
                    ready_coro.push_back(next);
                }
            }
            CoroutineStatus::Yield(yield_msg) => match yield_msg {
                WaitingReason::Tick => self.waiting_on_tick.push_back(coro_id),
                WaitingReason::Duration(d) => self.waiting_on_time.push_back((coro_id, d)),
                WaitingReason::First(handlers) => {
                    self.waiting_first.insert(coro_id, handlers.clone());
                    for handler in handlers.iter() {
                        self.is_awaited_by
                            .insert(Entity::from_bits(handler), Some(coro_id));
                    }

                    match handlers.iter().find(|h| {
                        let id = Entity::from_bits(*h);
                        !self.done_but_not_awaited.contains(&id)
                            && !self.coroutines.contains_key(&id)
                    }) {
                        Some(_) => {
                            self.cancel(coro_id, world);
                        }
                        _ => {
                            if let Some(h) = handlers
                                .iter()
                                .find(|h| self.done_but_not_awaited.remove(&Entity::from_bits(*h)))
                            {
                                let id = Entity::from_bits(h);
                                let node = run_cx.parent_table.add_child(this_node, id);
                                if let Some(next) = self.mark_as_done(id, node, world, run_cx) {
                                    ready_coro.push_back(next);
                                }
                            }
                        }
                    }
                }
                WaitingReason::All(handlers) => {
                    self.waiting_all.insert(coro_id, handlers.clone());
                    for handler in handlers.iter() {
                        self.is_awaited_by
                            .insert(Entity::from_bits(handler), Some(coro_id));
                    }

                    match handlers.iter().find(|h| {
                        let id = Entity::from_bits(*h);
                        !self.done_but_not_awaited.contains(&id)
                            && !self.coroutines.contains_key(&id)
                    }) {
                        Some(_) => {
                            self.cancel(coro_id, world);
                        }
                        _ => {
                            for h in handlers.iter() {
                                let id = Entity::from_bits(h);
                                if self.done_but_not_awaited.remove(&id) {
                                    let node = run_cx.parent_table.add_child(this_node, id);
                                    if let Some(next) = self.mark_as_done(id, node, world, run_cx) {
                                        ready_coro.push_back(next);
                                    }
                                }
                            }
                        }
                    }
                }
            },
        };
    }

    /// Mark a coroutine as done, and properly handle cleanup. It returns Some(_) if a
    /// coroutine was wating on this one, and can now make progress.
    fn mark_as_done(
        &mut self,
        coro_id: Entity,
        coro_node: usize,
        world: &mut World,
        run_cx: &mut RunContext,
    ) -> Option<(Entity, usize)> {
        self.coroutines.remove(&coro_id);

        if let Some(children) = self.scope_ownership.remove(&coro_id) {
            for c in children {
                self.cancel(Entity::from_bits(c), world);
            }
        }

        match self.is_awaited_by.remove(&coro_id) {
            Some(Some(parent)) => {
                if let Some(others) = self.waiting_first.remove(&parent) {
                    // coro is the "winner", all the others are cancelled
                    for o in others {
                        let id = Entity::from_bits(o);
                        self.is_awaited_by.remove(&id);
                        self.cancel(id, world);
                    }
                    let node = run_cx.parent_table.add_child(coro_node, parent);
                    return Some((parent, node));
                }
                if let Some(others) = self.waiting_all.get_mut(&parent) {
                    others.remove(coro_id.to_bits());

                    let node = match run_cx.parent_table.node_map.get(&parent).copied() {
                        Some(node) => {
                            run_cx.parent_table.add_parent(coro_node, node);
                            node
                        }
                        None => run_cx.parent_table.add_child(coro_node, parent),
                    };
                    if others.is_empty() {
                        return Some((parent, node));
                    }
                }
                None
            }
            Some(None) => {
                self.done_but_not_awaited.insert(coro_id);
                None
            }
            None => {
                world.despawn(coro_id);
                None
            }
        }
    }
}

#[derive(Default)]
struct ParentTable {
    table: Vec<SetUsize>,
    node_map: HashMap<Entity, usize>,
}

impl ParentTable {
    fn new() -> Self {
        Self::default()
    }

    /// Add a child node for coroutine `child`, to the `parent` node.
    /// If `child` already has a node, then this function behaves like
    /// [`add_parent`].
    fn add_child(&mut self, parent: usize, child: Entity) -> usize {
        match self.node_map.get(&child).copied() {
            Some(node) => {
                self.add_parent(parent, node);
                node
            }
            None => {
                let mut parents = self.table.get(parent).unwrap().clone();
                parents.insert(parent);
                self.table.push(parents);
                let node = self.table.len() - 1;
                self.node_map.insert(child, node);
                node
            }
        }
    }

    fn add_root(&mut self, c: Entity) -> usize {
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

#[derive(Default)]
struct RunContext {
    parent_table: ParentTable,
}
