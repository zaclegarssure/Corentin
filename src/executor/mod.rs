use bevy::utils::HashMap;
use bevy::{ecs::component::ComponentId, prelude::*};
use coroutine::WaitingReason;
use std::collections::VecDeque;

use crate::coroutine;
use crate::coroutine::observable::ObservableId;
use crate::coroutine::{CoroObject, CoroWrites, Coroutine, CoroutineResult};

mod run_ctx;
pub(crate) use run_ctx::RunContext;

type CoroId = u32;

#[derive(Resource)]
pub struct Executor {
    coroutines: HashMap<CoroId, CoroObject>,
    waiting_on_tick: VecDeque<CoroId>,
    waiting_on_duration: VecDeque<(Timer, CoroId)>,
    waiting_on_change: HashMap<(Entity, ComponentId), Vec<CoroId>>,
    waiting_on_par_or: HashMap<CoroId, Vec<CoroId>>,
    waiting_on_par_and: HashMap<CoroId, Vec<CoroId>>,
    is_awaited_by: HashMap<CoroId, CoroId>,
    next_id: CoroId,
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: This is safe because the only !Send and !Sync field (receiver) is only accessed
// when calling run(), which is done in a single threaded context.
unsafe impl Send for Executor {}
unsafe impl Sync for Executor {}

impl Executor {
    pub fn new() -> Self {
        Executor {
            coroutines: HashMap::new(),
            waiting_on_tick: VecDeque::new(),
            waiting_on_duration: VecDeque::new(),
            waiting_on_change: HashMap::new(),
            waiting_on_par_or: HashMap::new(),
            waiting_on_par_and: HashMap::new(),
            is_awaited_by: HashMap::new(),
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
    pub fn add(&mut self, coroutine: CoroObject) {
        let id = self.next_id();
        self.coroutines.insert(id, coroutine);
        self.waiting_on_tick.push_back(id);
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

        let mut run_cx = RunContext::new();
        let mut ready_coro: VecDeque<(CoroId, usize)> = root_coros
            .into_iter()
            .map(|c_id| (c_id, run_cx.parent_table.add_root()))
            .collect();

        while let Some((c_id, node)) = ready_coro.pop_front() {
            self.run(c_id, node, &mut run_cx, &mut ready_coro, world);
        }
    }

    /// Run a specific coroutine and it's children.
    fn run(
        &mut self,
        coro_id: CoroId,
        this_node: usize,
        run_cx: &mut RunContext,
        ready_coro: &mut VecDeque<(CoroId, usize)>,
        world: &mut World,
    ) {
        if !self.coroutines.contains_key(&coro_id) {
            return;
        }

        let coro = self.coroutines.get_mut(&coro_id).unwrap().get();
        if !coro.as_mut().is_valid(world) {
            self.cancel(coro_id);
            return;
        }

        let result = Coroutine::resume(coro.as_mut(), world);

        let mut writes = world.resource_mut::<CoroWrites>();

        while let Some((write, c_id)) = writes.0.pop_front() {
            run_cx.write_table.insert((write, c_id), this_node);
            if let Some(nexts) = self.waiting_on_change.remove(&(write, c_id)) {
                for c in nexts {
                    let node = match run_cx.current_node_map.remove(&c) {
                        Some(n) => {
                            run_cx.parent_table.add_parent(this_node, n);
                            n
                        }
                        None => run_cx.parent_table.add_child(this_node),
                    };
                    ready_coro.push_back((c, node));
                }
            }
        }

        match result {
            CoroutineResult::Done(_) => {
                self.coroutines.remove(&coro_id);
                if let Some(parent) = self.is_awaited_by.remove(&coro_id) {
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
                        let index = others.iter().position(|c| *c == coro_id).unwrap();
                        others.remove(index);
                        if others.is_empty() {
                            // TODO: should run immediatly
                            self.waiting_on_tick.push_back(parent);
                            self.waiting_on_par_and.remove(&parent);
                        }
                    }
                }
            }
            CoroutineResult::Yield(yield_msg) => match yield_msg {
                WaitingReason::Tick => self.waiting_on_tick.push_back(coro_id),
                WaitingReason::Duration(d) => self.waiting_on_duration.push_back((d, coro_id)),
                WaitingReason::Changed(change_id) => match change_id {
                    ObservableId::Component(from, component) => {
                        let next_node = run_cx.parent_table.add_child(this_node);
                        match run_cx.can_execute_now(next_node, &(from, component)) {
                            Some(writer) => {
                                run_cx.parent_table.add_parent(writer, next_node);
                                ready_coro.push_back((coro_id, next_node));
                            }
                            None => {
                                self.waiting_on_change
                                    .entry((from, component))
                                    .or_default()
                                    .push(coro_id);
                                run_cx.current_node_map.insert(coro_id, next_node);
                            }
                        }
                    }
                    _ => todo!(),
                },
                WaitingReason::ParOr { coroutines } => {
                    let parent = coro_id;
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
                        ready_coro.push_back((id, node));
                    }
                }
                WaitingReason::ParAnd { coroutines } => {
                    let parent = coro_id;
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
                        ready_coro.push_back((id, node));
                    }
                }
            },
        };
    }
}

#[cfg(test)]
mod tests {
    use crate::{coroutine::CoroWrites, prelude::*};

    use super::Executor;
    use bevy::{
        ecs::system::EntityCommand,
        prelude::{Component, Mut, World},
        time::Time,
    };
    use std::time::Instant;

    #[derive(Component)]
    struct ExampleComponent;

    #[derive(Component)]
    struct ExampleComponentBar;

    #[test]
    fn par_or_despawn_correctly() {
        let mut world = World::new();
        let e = world.spawn_empty().id();
        world.init_resource::<Executor>();
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));

        coroutine(|fib: Fib| async move {
            fib.par_or(|fib: Fib| async move {
                fib.par_or(|fib: Fib| async move {
                    loop {
                        fib.next_tick().await;
                    }
                })
                .with(|fib: Fib| async move {
                    loop {
                        fib.next_tick().await;
                    }
                })
                .await;
            })
            .with(|fib: Fib| async move {
                for _ in 0..4 {
                    fib.next_tick().await;
                }
            })
            .await;
        })
        .apply(e, &mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
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

    #[test]
    fn waiting_on_change_cancel_when_components_not_present() {
        let mut world = World::new();
        world.init_resource::<Executor>();
        world.init_resource::<CoroWrites>();
        world.insert_resource(Time::new(Instant::now()));
        let e = world.spawn((ExampleComponent, ExampleComponentBar)).id();

        coroutine(|fib: Fib, _ex: Rd<ExampleComponent>| async move {
            loop {
                fib.next_tick().await;
            }
        })
        .apply(e, &mut world);

        coroutine(|fib: Fib, _ex: Rd<ExampleComponentBar>| async move {
            loop {
                fib.next_tick().await;
            }
        })
        .apply(e, &mut world);

        world.resource_scope(|w, mut executor: Mut<Executor>| {
            assert_eq!(executor.coroutines.len(), 2);
            executor.tick(w);
            assert_eq!(executor.coroutines.len(), 2);
            w.entity_mut(e).remove::<ExampleComponent>();
            executor.tick(w);
            assert_eq!(executor.coroutines.len(), 1);
            w.entity_mut(e).despawn();
            executor.tick(w);
            assert_eq!(executor.coroutines.len(), 0);
        });
    }

    //#[test]
    //fn waiting_on_change_dont_cancel_on_optional() {
    //    let mut world = World::new();
    //    world.init_resource::<Executor>();
    //    world.init_resource::<CoroWrites>();
    //    world.insert_resource(Time::new(Instant::now()));
    //    let e = world.spawn(ExampleComponent).id();
    //    coroutine(
    //        |mut fib: Fib, _ex: Option<R<ExampleComponent>>| async move {
    //            loop {
    //                fib.next_tick().await;
    //            }
    //        },
    //    )
    //    .apply(e, &mut world);
    //    world.resource_scope(|w, mut executor: Mut<Executor>| {
    //        assert_eq!(executor.coroutines.len(), 1);
    //        executor.tick(w);
    //        assert_eq!(executor.coroutines.len(), 1);
    //        w.entity_mut(e).remove::<ExampleComponent>();
    //        executor.tick(w);
    //        assert_eq!(executor.coroutines.len(), 1);
    //    });
    //}
}
