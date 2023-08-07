use bevy::ecs::component::Tick;
use bevy::{ecs::system::SystemState, prelude::*};
use core::task::Context;
use coroutine::{OwnedBy, CoroState, Fib, WaitingReason, WaitingState};
use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;
use waker::create;

pub mod coroutine;
mod waker;

type CoroObject = Pin<Box<dyn Future<Output = ()>>>;

pub struct Executor {
    coroutines: HashMap<Entity, CoroObject>,
    receiver: Rc<Cell<Option<WaitingReason>>>,
    last_tick: Option<Tick>,
    added: VecDeque<(CoroObject, Option<Entity>)>,
}

impl Executor {
    pub fn new() -> Self {
        Executor {
            coroutines: HashMap::new(),
            receiver: Rc::new(Cell::new(None)),
            last_tick: None,
            added: VecDeque::new(),
        }
    }

    pub fn add<C, F>(&mut self, closure: C)
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.receiver),
        };
        self.added.push_back((Box::pin(closure(fib)), None));
    }

    pub fn add_to_entity<C, F>(&mut self, entity: Entity, closure: C)
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib, Entity) -> F,
    {
        let fib = Fib {
            state: CoroState::Running,
            sender: Rc::clone(&self.receiver),
        };

        self.added
            .push_back((Box::pin(closure(fib, entity)), Some(entity)));
    }

    pub fn run(&mut self, world: &mut World) {
        while let Some((coroutine, owner)) = self.added.pop_front() {
            match owner {
                Some(owner) => {
                    let id = world.spawn((WaitingState::Ready, OwnedBy(owner))).id();
                    self.coroutines.insert(id, coroutine);
                }
                None => {
                    let id = world.spawn(WaitingState::Ready).id();
                    self.coroutines.insert(id, coroutine);
                }
            }
        }

        let waker = create();
        let mut context = Context::from_waker(&waker);

        let curr_tick = world.change_tick();
        let last_tick = self.last_tick.unwrap_or(curr_tick);

        let mut sys_state: SystemState<(Query<(Entity, &mut WaitingState)>, Res<Time>, Commands)> =
            SystemState::new(world);
        // Not needed since we always recreate the query (which is not that great but we will cache it later on)
        //sys_state.update_archetypes(world);

        let mut ready = vec![];

        // Safety: we will only mutate WaitingState, and read from other components in the WaitOnChange case,
        // which cannot collide, simply by checking it at runtime.
        unsafe {
            let world_cell = world.as_unsafe_world_cell();
            let (mut q, time, mut commands) = sys_state.get_unchecked_manual(world_cell);
            for (entity, mut state) in q.iter_mut() {
                match *state {
                    WaitingState::WaitOnTick => {
                        *state = WaitingState::Ready;
                        ready.push(entity);
                    }
                    WaitingState::WaitOnDuration(ref mut t) => {
                        t.tick(time.delta());
                        if t.just_finished() {
                            *state = WaitingState::Ready;
                            ready.push(entity);
                        }
                    }
                    // Todo find something better than polling maybe ?
                    WaitingState::WaitOnChange { from, component_id } => {
                        // For safety reasons, since we may have a &mut to WaitingState
                        assert!(
                            component_id
                                != world_cell
                                    .components()
                                    .component_id::<WaitingState>()
                                    .unwrap()
                        );
                        match world_cell.get_entity(from) {
                            Some(e) => match e.get_change_ticks_by_id(component_id) {
                                Some(t) => {
                                    if t.last_changed_tick().is_newer_than(last_tick, curr_tick) {
                                        *state = WaitingState::Ready;
                                        ready.push(entity);
                                    }
                                }
                                // Damn I wish we had relations
                                None => {
                                    commands.entity(entity).despawn();
                                }
                            },
                            None => {
                                commands.entity(entity).despawn();
                            }
                        };
                    }
                    WaitingState::Ready => ready.push(entity),
                };
            }
        }

        sys_state.apply(world);
        self.last_tick = Some(curr_tick);

        for coroutine_id in ready {
            match self
                .coroutines
                .get_mut(&coroutine_id)
                .unwrap()
                .as_mut()
                .poll(&mut context)
            {
                Poll::Pending => {
                    let msg = self
                        .receiver
                        .replace(None)
                        .expect("A coroutine yielded without any reasons");
                    let state = WaitingState::from_reason(msg, world);
                    world.entity_mut(coroutine_id).insert(state);
                }
                Poll::Ready(_) => {
                    world.despawn(coroutine_id);
                }
            }
        }

        // TODO: Think of something more efficient
        self.coroutines
            .retain(|id, _| world.get_entity(*id).is_some());
    }
}
