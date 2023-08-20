use bevy::utils::HashMap;
use bevy::{ecs::component::ComponentId, prelude::*};
use core::task::Context;
use coroutine::{Fib, WaitingReason};
use std::cell::Cell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;
use waker::create;

use crate::{coroutine, waker};
use coroutine::grab::GrabReason;
use msg_channel::Receiver;

pub(crate) type CoroObject = Pin<Box<dyn Future<Output = ()> + Send + Sync>>;

pub(crate) mod msg_channel;
mod run_ctx;
use run_ctx::RunContext;

use self::run_ctx::SuspendedCoro;

type CoroId = u32;

#[derive(Resource)]
pub struct Executor {
    coroutines: HashMap<CoroId, CoroObject>,
    yield_msg: Receiver<WaitingReason>,
    grab_msg: Receiver<GrabReason>,
    waiting_on_tick: VecDeque<CoroId>,
    waiting_on_duration: VecDeque<(Timer, CoroId)>,
    waiting_on_change: HashMap<(Entity, ComponentId), Vec<CoroId>>,
    waiting_on_par_or: HashMap<CoroId, Vec<CoroId>>,
    waiting_on_par_and: HashMap<CoroId, Vec<CoroId>>,
    is_awaited_by: HashMap<CoroId, CoroId>,
    own: HashMap<Entity, Vec<CoroId>>,
    world_window: Rc<Cell<Option<*mut World>>>,
    accesses: HashMap<CoroId, GrabReason>,
    next_id: CoroId,
}

// SAFETY: This is safe because the only !Send and !Sync field (receiver) is only accessed
// when calling run(), which is done in a single threaded context.
unsafe impl Send for Executor {}
unsafe impl Sync for Executor {}

const ERR_WRONGAWAIT: &str = "A coroutine yielded without notifying the executor
the reason. That is most likely because it awaits a
future which is not part of this library.";

impl Executor {
    pub fn new() -> Self {
        Executor {
            coroutines: HashMap::new(),
            yield_msg: Receiver::new(),
            grab_msg: Receiver::new(),
            waiting_on_tick: VecDeque::new(),
            waiting_on_duration: VecDeque::new(),
            waiting_on_change: HashMap::new(),
            waiting_on_par_or: HashMap::new(),
            waiting_on_par_and: HashMap::new(),
            is_awaited_by: HashMap::new(),
            own: HashMap::new(),
            world_window: Rc::new(Cell::new(None)),
            accesses: HashMap::new(),
            next_id: 0,
        }
    }

    fn next_id(&mut self) -> CoroId {
        // Look at how Entity work, to reuse Ids
        let res = self.next_id;
        self.next_id += 1;
        res
    }

    /// Add a coroutine to the executor.
    pub fn add<C, F>(&mut self, closure: C)
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            yield_sender: self.yield_msg.sender(),
            grab_sender: self.grab_msg.sender(),
            owner: None,
            world_window: Rc::clone(&self.world_window),
        };
        let id = self.next_id();
        self.coroutines.insert(id, Box::pin(closure(fib)));
        self.waiting_on_tick.push_back(id);
    }

    /// Add a coroutine owned by an [`Entity`] to the executor.
    /// If the entity is removed, the coroutine is dropped.
    pub fn add_to_entity<C, F>(&mut self, entity: Entity, closure: C)
    where
        F: Future<Output = ()> + 'static + Send + Sync,
        C: FnOnce(Fib, Entity) -> F,
    {
        let fib = Fib {
            yield_sender: self.yield_msg.sender(),
            grab_sender: self.grab_msg.sender(),
            owner: None,
            world_window: Rc::clone(&self.world_window),
        };
        let id = self.next_id();
        self.coroutines.insert(id, Box::pin(closure(fib, entity)));
        self.waiting_on_tick.push_back(id);
        self.own.entry(entity).or_default().push(id);
    }

    /// Drop a coroutine from the executor.
    pub fn cancel(&mut self, coroutine: CoroId) {
        self.coroutines.remove(&coroutine);

        if let Some(others) = self.waiting_on_par_or.remove(&coroutine) {
            for o in others {
                self.cancel(o);
            }
        }
        if let Some(others) = self.waiting_on_par_and.remove(&coroutine) {
            for o in others {
                self.cancel(o);
            }
        }

        if let Some(parent) = self.is_awaited_by.remove(&coroutine) {
            // In case of a ParOr that might be not what's always needed
            // But in any case this API will be reworked if I can figure out
            // how to "inline" ParOr and ParAnd
            self.cancel(parent);
        }
    }

    /// Run the executor until no coroutine can progress anymore.
    /// Should generally be called once per frame.
    pub fn tick(&mut self, world: &mut World) {
        let mut root_coros = VecDeque::<CoroId>::new();

        root_coros.append(&mut self.waiting_on_tick);

        world.resource_scope(|w, time: Mut<Time>| {
            // Tick all coroutines waiting on duration
            for (t, _) in self.waiting_on_duration.iter_mut() {
                t.tick(time.delta());
            }
            self.waiting_on_duration.retain(|(t, coro)| {
                if t.just_finished() {
                    root_coros.push_back(*coro);
                }
                !t.just_finished()
            });

            let mut to_despawn = vec![];

            // Check all coroutines waiting on change, this is basically a polling approach,
            // I will study later on a better way
            self.waiting_on_change.retain(|(e, c), coro| {
                if let Some(e) = w.get_entity(*e) {
                    if let Some(t) = e.get_change_ticks_by_id(*c) {
                        // TODO: Make sure this is correct, I'm really not that confident, even though it works with a simple example
                        if t.is_changed(w.last_change_tick(), w.change_tick()) {
                            for c in coro {
                                root_coros.push_back(*c);
                            }
                            return false;
                        }
                        return true;
                    }
                }
                to_despawn.append(coro);
                false
            });

            for c in to_despawn {
                self.cancel(c);
            }
        });

        let mut run_cx: RunContext = RunContext::new();
        let waker = create();
        let mut context = Context::from_waker(&waker);

        // TODO: Find maybe a safer abstraction ?
        self.world_window.replace(Some(world as *mut _));

        while let Some(coro) = root_coros.pop_front() {
            let node = run_cx.parent_table.add_root();
            self.run(coro, node, false, &mut run_cx, &mut context);
        }

        while let Some(SuspendedCoro { coro_id, node }) = run_cx.delayed.pop_front() {
            self.run(coro_id, node, true, &mut run_cx, &mut context);
        }
    }

    /// Run a specific coroutine and it's children.
    fn run(
        &mut self,
        coro: CoroId,
        this_node: usize,
        resumed: bool,
        run_cx: &mut RunContext,
        cx: &mut Context,
    ) {
        if !self.coroutines.contains_key(&coro) {
            return;
        }

        let accesses = self.accesses.get(&coro);
        if let Some(accesses) = accesses {
            let mut has_conflicts = false;
            let conflicters = run_cx.write_table.conflicts(accesses.writes());
            for c in conflicters {
                if !resumed || !run_cx.parent_table.is_parent(*c, this_node) {
                    run_cx.parent_table.add_parent(*c, this_node);
                    has_conflicts = true;
                }
            }

            if has_conflicts {
                run_cx
                    .delayed
                    .push_back(SuspendedCoro::new(coro, this_node));

                return;
            }
        }

        let result = self.coroutines.get_mut(&coro).unwrap().as_mut().poll(cx);
        let yield_msg = self.yield_msg.receive();
        let next_access = self.grab_msg.receive();

        if let Some(accesses) = accesses {
            // SAFETY: No other coroutines a running right now, we have exclusive world access
            let world = unsafe { &mut *self.world_window.get().unwrap() };
            // TODO: Should probably find a better way
            for w in accesses.writes().iter().filter(|(e, cid)| {
                let tick = world
                    .get_entity(*e)
                    .unwrap()
                    .get_change_ticks_by_id(*cid)
                    .unwrap();
                // TODO: Check if it is correct (Looks okay enough)
                tick.is_changed(world.last_change_tick(), world.change_tick())
            }) {
                run_cx.write_table.insert(*w, this_node);
                if let Some(nexts) = self.waiting_on_change.remove(w) {
                    for c in nexts {
                        let node = match run_cx.current_node_map.remove(&c) {
                            Some(n) => {
                                run_cx.parent_table.add_parent(this_node, n);
                                n
                            }
                            None => run_cx.parent_table.add_child(this_node),
                        };
                        self.run(c, node, false, run_cx, cx);
                    }
                }
            }
        }

        match result {
            Poll::Ready(_) => {
                self.coroutines.remove(&coro);
                if let Some(parent) = self.is_awaited_by.remove(&coro) {
                    if let Some(others) = self.waiting_on_par_or.remove(&parent) {
                        // coro is the "winner", all the others are cancelled
                        for o in others {
                            self.is_awaited_by.remove(&o);
                            self.cancel(o);
                        }
                        // TODO: should run immediatly
                        self.waiting_on_tick.push_back(parent);
                    }
                    if let Some(others) = self.waiting_on_par_and.get_mut(&parent) {
                        let index = others.iter().position(|c| *c == coro).unwrap();
                        others.remove(index);
                        if others.is_empty() {
                            // TODO: should run immediatly
                            self.waiting_on_tick.push_back(parent);
                            self.waiting_on_par_and.remove(&parent);
                        }
                    }
                }
            }
            Poll::Pending => {
                if let Some(grab) = next_access {
                    self.accesses.insert(coro, grab);
                }
                match yield_msg.expect(ERR_WRONGAWAIT) {
                    WaitingReason::Tick => self.waiting_on_tick.push_back(coro),
                    WaitingReason::Duration(d) => self.waiting_on_duration.push_back((d, coro)),
                    WaitingReason::Changed { from, component } => {
                        let next_node = run_cx.parent_table.add_child(this_node);
                        match run_cx.can_execute_now(next_node, &(from, component)) {
                            Some(writer) => {
                                run_cx.parent_table.add_parent(writer, next_node);
                                self.run(coro, next_node, false, run_cx, cx);
                            }
                            None => {
                                self.waiting_on_change
                                    .entry((from, component))
                                    .or_default()
                                    .push(coro);
                                run_cx.current_node_map.insert(coro, next_node);
                            }
                        }
                    }
                    WaitingReason::ParOr { coroutines } => {
                        let parent = coro;
                        let mut all_ids = Vec::with_capacity(coroutines.len());
                        for coroutine in coroutines {
                            let id = self.next_id();
                            self.coroutines.insert(id, coroutine);
                            self.is_awaited_by.insert(id, parent);
                            all_ids.push(id);
                        }

                        self.waiting_on_par_or.insert(parent, all_ids.clone());

                        for id in all_ids {
                            let node = run_cx.parent_table.add_child(this_node);
                            self.run(id, node, false, run_cx, cx);
                        }
                    }
                    WaitingReason::ParAnd { coroutines } => {
                        let parent = coro;
                        let mut all_ids = Vec::with_capacity(coroutines.len());
                        for coroutine in coroutines {
                            let id = self.next_id();
                            self.coroutines.insert(id, coroutine);
                            self.is_awaited_by.insert(id, parent);
                            all_ids.push(id);
                        }

                        self.waiting_on_par_and.insert(parent, all_ids.clone());

                        for id in all_ids {
                            let node = run_cx.parent_table.add_child(this_node);
                            self.run(id, node, false, run_cx, cx);
                        }
                    }
                }
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::Executor;
    use bevy::{
        prelude::{Mut, World},
        time::Time,
    };
    use std::time::Instant;

    #[test]
    fn par_or_despawn_correctly() {
        let mut world = World::new();
        world.insert_resource(Executor::new());
        world.insert_resource(Time::new(Instant::now()));
        world.resource_scope(|w, mut executor: Mut<Executor>| {
            executor.add(move |mut fib| async move {
                fib.par_or(|mut fib| async move {
                    fib.par_or(|mut fib| async move {
                        loop {
                            fib.next_tick().await;
                        }
                    })
                    .with(|mut fib| async move {
                        loop {
                            fib.next_tick().await;
                        }
                    })
                    .await;
                })
                .with(|mut fib| async move {
                    for _ in 0..4 {
                        fib.next_tick().await;
                    }
                })
                .await;
            });

            assert_eq!(executor.is_awaited_by.len(), 0);
            assert_eq!(executor.waiting_on_par_or.len(), 0);
            assert_eq!(executor.coroutines.len(), 1);

            executor.tick(w);
            assert_eq!(executor.is_awaited_by.len(), 4);
            assert_eq!(executor.waiting_on_par_or.len(), 2);
            assert_eq!(executor.coroutines.len(), 5);

            // Need 1 extra tick, because of the 1 tick delayed after a ParAnd/ParOr is completed (will be fixed)
            for _ in 0..5 {
                executor.tick(w);
            }

            assert_eq!(executor.is_awaited_by.len(), 0);
            assert_eq!(executor.waiting_on_par_or.len(), 0);
            assert_eq!(executor.coroutines.len(), 0);
        });
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}
