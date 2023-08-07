use bevy::ecs::component::Tick;
use bevy::{ecs::system::SystemState, prelude::*};
use core::task::Context;
use coroutine::{BoundTo, CoroState, Fib, WaitingState, WaitingReason};
use std::cell::Cell;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Poll;
use waker::create;

mod coroutine;
mod waker;

type CoroObject = Pin<Box<dyn Future<Output = ()>>>;

pub struct Executor {
    coroutines: HashMap<Entity, CoroObject>,
    receiver: Rc<Cell<Option<WaitingReason>>>,
    last_tick: Option<Tick>,
}

impl Executor {
    pub fn new() -> Self {
        Executor {
            coroutines: HashMap::new(),
            receiver: Rc::new(Cell::new(None)),
            last_tick: None,
        }
    }

    pub fn add<C, F>(&mut self, mut c: Commands, closure: C)
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib) -> F,
    {
        let id = c.spawn(WaitingState::Ready).id();
        let fib = Fib {
            state: CoroState::Running,
            id,
            sender: Rc::clone(&self.receiver),
        };

        self.coroutines.insert(id, Box::pin(closure(fib)));
    }

    pub fn add_to_entity<C, F>(&mut self, mut c: Commands, entity: Entity, closure: C)
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib) -> F,
    {
        let id = c.spawn((WaitingState::Ready, BoundTo(entity))).id();
        let fib = Fib {
            state: CoroState::Running,
            id,
            sender: Rc::clone(&self.receiver),
        };

        self.coroutines.insert(id, Box::pin(closure(fib)));
    }

    pub fn run(&mut self, world: &mut World) {
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

            self.last_tick = Some(curr_tick);
        }

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
    }
}
