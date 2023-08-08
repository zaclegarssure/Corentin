use bevy::{ecs::component::ComponentId, prelude::*};
use core::task::Context;
use coroutine::{CoroState, Fib, WaitingReason};
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;
use waker::create;

use crate::{coroutine, waker};

pub(crate) type CoroObject = Pin<Box<dyn Future<Output = ()>>>;

type CoroId = Entity;

pub struct Executor {
    coroutines: HashMap<Entity, CoroObject>,
    receiver: Rc<Cell<Option<WaitingReason>>>,
    added: VecDeque<(CoroObject, Option<Entity>)>,
    ready: VecDeque<CoroId>,
    waiting_on_tick: VecDeque<CoroId>,
    waiting_on_duration: VecDeque<(Timer, CoroId)>,
    waiting_on_change: HashMap<(Entity, ComponentId), Vec<CoroId>>,
    waiting_on_par_or: HashMap<CoroId, Vec<CoroId>>,
    waiting_on_par_and: HashMap<CoroId, Vec<CoroId>>,
    is_awaited_by: HashMap<CoroId, CoroId>,
    own: HashMap<Entity, Vec<CoroId>>,
}

const ERR_WRONGAWAIT: &'static str = "A coroutine yielded without notifying the executor
the reason. That is most likely because it awaits a
future which is not part of this library.";

impl Executor {
    pub fn new() -> Self {
        Executor {
            coroutines: HashMap::new(),
            receiver: Rc::new(Cell::new(None)),
            added: VecDeque::new(),
            ready: VecDeque::new(),
            waiting_on_tick: VecDeque::new(),
            waiting_on_duration: VecDeque::new(),
            waiting_on_change: HashMap::new(),
            waiting_on_par_or: HashMap::new(),
            waiting_on_par_and: HashMap::new(),
            is_awaited_by: HashMap::new(),
            own: HashMap::new(),
            //waiting_
        }
    }

    /// Add a coroutine to the executor.
    pub fn add<C, F>(&mut self, closure: C)
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.receiver),
            owner: None,
        };
        self.added.push_back((Box::pin(closure(fib)), None));
    }

    /// Add a coroutine owned by an [`Entity`] to the executor.
    /// If the entity is removed, the coroutine is dropped.
    pub fn add_to_entity<C, F>(&mut self, entity: Entity, closure: C)
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib, Entity) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.receiver),
            owner: Some(entity),
        };

        self.added
            .push_back((Box::pin(closure(fib, entity)), Some(entity)));
    }

    /// Drop a coroutine from the executor.
    pub fn cancel(&mut self, world: &mut World, coroutine: CoroId) {
        if !self.coroutines.contains_key(&coroutine) {
            return;
        }

        self.coroutines.remove(&coroutine);

        if let Some(others) = self.waiting_on_par_or.remove(&coroutine) {
            for o in others {
                self.cancel(world, o);
            }
        }
        if let Some(others) = self.waiting_on_par_and.remove(&coroutine) {
            for o in others {
                self.cancel(world, o);
            }
        }

        if let Some(parent) = self.is_awaited_by.remove(&coroutine) {
            // In case of a ParOr that might be not what's always needed
            // But in any case this API will be reworked if I can figure out
            // how to "inline" ParOr and ParAnd
            self.cancel(world, parent);
        }

        world.despawn(coroutine);
    }

    /// Run the executor until no coroutine can progress anymore.
    /// Should generally be called once per frame.
    pub fn run(&mut self, world: &mut World) {
        // Add new coroutines
        while let Some((coroutine, owner)) = self.added.pop_front() {
            // We just use entity as a convenient unique ID
            // Maybe in the future we will store everything in the ECS, but without
            // relation that does not seem like a good fit
            let id = world.spawn_empty().id();
            self.coroutines.insert(id, coroutine);
            self.ready.push_back(id);
            if let Some(owner) = owner {
                match self.own.get_mut(&owner) {
                    Some(owned) => owned.push(id),
                    None => {
                        self.own.insert(owner, vec![id]);
                    }
                }
            }
        }

        let waker = create();
        let mut context = Context::from_waker(&waker);

        self.ready.append(&mut self.waiting_on_tick);

        world.resource_scope(|w, time: Mut<Time>| {
            // Tick all coroutines waiting on duration
            for (t, _) in self.waiting_on_duration.iter_mut() {
                t.tick(time.delta());
            }
            self.waiting_on_duration.retain(|(t, coro)| {
                if t.just_finished() {
                    self.ready.push_back(*coro);
                }
                !t.just_finished()
            });

            let mut to_despawn = vec![];

            // Check all coroutines waiting on change
            self.waiting_on_change.retain(|(e, c), coro| {
                if let Some(e) = w.get_entity(*e) {
                    if let Some(t) = e.get_change_ticks_by_id(*c) {
                        // TODO: Make sure this is correct, I'm really not that confident, even though it works with a simple example
                        if t.is_changed(w.last_change_tick(), w.change_tick()) {
                            for c in coro {
                                self.ready.push_back(*c);
                            }
                            return false;
                        }
                        return true;
                    }
                }
                to_despawn.append(coro);
                return false;
            });

            for c in to_despawn {
                self.cancel(w, c);
            }
        });

        // Run the coroutines
        while let Some(coro) = self.ready.pop_front() {
            if !self.coroutines.contains_key(&coro) {
                continue;
            }

            match self
                .coroutines
                .get_mut(&coro)
                .unwrap()
                .as_mut()
                .poll(&mut context)
            {
                Poll::Pending => {
                    let msg = self.receiver.replace(None).expect(ERR_WRONGAWAIT);
                    match msg {
                        WaitingReason::WaitOnTick => self.waiting_on_tick.push_back(coro),
                        WaitingReason::WaitOnDuration(d) => {
                            self.waiting_on_duration.push_back((d, coro))
                        }
                        WaitingReason::WaitOnChange { from, type_id } => {
                            let component_id = world.components().get_id(type_id).unwrap();
                            self.waiting_on_change
                                .entry((from, component_id))
                                .or_insert_with(Vec::new)
                                .push(coro);
                        }
                        WaitingReason::WaitOnParOr { coroutines } => {
                            let parent = coro;
                            let mut all_ids = Vec::with_capacity(coroutines.len());
                            for coroutine in coroutines {
                                let id = world.spawn_empty().id();
                                self.coroutines.insert(id, coroutine);
                                self.ready.push_back(id);
                                self.is_awaited_by.insert(id, parent);
                                all_ids.push(id);
                            }
                            let prev = self.waiting_on_par_or.insert(parent, all_ids);
                            assert!(prev.is_none());
                        }
                        WaitingReason::WaitOnParAnd { coroutines } => {
                            let parent = coro;
                            let mut all_ids = Vec::with_capacity(coroutines.len());
                            for coroutine in coroutines {
                                let id = world.spawn_empty().id();
                                self.coroutines.insert(id, coroutine);
                                self.ready.push_back(id);
                                self.is_awaited_by.insert(id, parent);
                                all_ids.push(id);
                            }
                            let prev = self.waiting_on_par_and.insert(parent, all_ids);
                            assert!(prev.is_none());
                        }
                    };
                }
                Poll::Ready(_) => {
                    match self.is_awaited_by.remove(&coro) {
                        Some(parent) => {
                            if let Some(others) = self.waiting_on_par_or.remove(&parent) {
                                // coro is the "winner", all the others are cancelled
                                for o in others {
                                    self.cancel(world, o);
                                }
                                self.ready.push_back(parent);
                            }
                            if let Some(others) = self.waiting_on_par_and.get_mut(&parent) {
                                let index = others.iter().position(|c| *c == coro).unwrap();
                                others.remove(index);
                                if others.len() == 0 {
                                    self.ready.push_back(parent);
                                    self.waiting_on_par_and.remove(&parent);
                                }
                                world.despawn(coro);
                                self.coroutines.remove(&coro);
                            }
                        }
                        None => {
                            world.despawn(coro);
                            self.coroutines.remove(&coro);
                        }
                    }
                }
            }
        }
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
        world.insert_non_send_resource(Executor::new());
        world.insert_resource(Time::new(Instant::now()));
        world.non_send_resource_scope(|w, mut executor: Mut<Executor>| {
            executor.add(move |mut fib| async move {
                fib.par_or(|mut fib| async move {
                    loop {
                        fib.next_tick().await;
                    }
                })
                .with(|mut fib| async move {
                    for _ in 0..4 {
                        fib.next_tick().await;
                    }
                })
                .await;
            });

            executor.run(w);
            assert_eq!(executor.is_awaited_by.len(), 2);
            assert_eq!(executor.waiting_on_par_or.len(), 1);
            assert_eq!(executor.coroutines.len(), 3);

            for _ in 0..5 {
                executor.run(w);
            }

            assert_eq!(executor.is_awaited_by.len(), 0);
            assert_eq!(executor.waiting_on_par_or.len(), 0);
            assert_eq!(executor.coroutines.len(), 0);
        });
    }
}
