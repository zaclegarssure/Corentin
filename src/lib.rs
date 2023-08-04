use bevy::ecs::world::WorldCell;
use bevy::prelude::*;
use core::cell::RefCell;
use core::task::Context;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::task::{RawWaker, RawWakerVTable, Waker};

#[derive(Clone, Copy)]
pub struct Duration {
    pub d: f32,
    pub unit: DurationUnit,
}

impl Duration {
    fn advance_by(&mut self, time: f32) {
        self.d -= time
            * match self.unit {
                DurationUnit::Milli => 1000.0,
                DurationUnit::Second => 1.0,
                DurationUnit::Minute => 1.0 / 60.0,
            }
    }
}

#[derive(Clone, Copy)]
pub enum DurationUnit {
    Milli,
    Second,
    Minute,
}

enum State {
    Halted,
    Running,
}

pub struct Fib {
    state: State,
    id: u32,
    waiting_next_tick: Rc<RefCell<VecDeque<u32>>>,
    waiting_on_duration: Rc<RefCell<VecDeque<(Duration, u32)>>>,
}

impl Fib {
    pub fn next_tick<'a>(&'a mut self) -> NextTick<'a> {
        NextTick { fib: self }
    }

    pub fn seconds<'a>(&'a mut self, duration: f32) -> DurationFuture<'a> {
        DurationFuture {
            fib: self,
            duration: Duration {
                d: duration,
                unit: DurationUnit::Second,
            },
        }
    }

    pub fn duration<'a>(&'a mut self, duration: Duration) -> DurationFuture<'a> {
        DurationFuture {
            fib: self,
            duration,
        }
    }

    pub fn with_component<'a, C, T>(&'a mut self, f: C) -> WithComponent<'a, T, C>
    where
        T: Component + Unpin,
        C: FnOnce(T) -> () + Unpin,
    {
        WithComponent {
            fib: self,
            closure: f,
            _phantom: PhantomData,
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct WithComponent<'a, T, C>
where
    T: Component + Unpin,
    C: FnOnce(T) -> () + Unpin,
{
    fib: &'a mut Fib,
    _phantom: PhantomData<T>,
    closure: C,
    //closure: Box<C>,
}

impl<'a, T, C> Future for WithComponent<'a, T, C>
where
    T: Component + Unpin,
    C: FnOnce(T) -> () + Unpin,
{
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        panic!("Not supported yet");
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct NextTick<'a> {
    fib: &'a mut Fib,
}

impl<'a> Future for NextTick<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.fib.state {
            // We assume the executor will only poll it once a new frame has beginned
            State::Halted => {
                self.fib.state = State::Running;
                Poll::Ready(())
            }
            State::Running => {
                self.fib.state = State::Halted;
                self.fib
                    .waiting_next_tick
                    .borrow_mut()
                    .push_back(self.fib.id);
                Poll::Pending
            }
        }
    }
}

#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct DurationFuture<'a> {
    fib: &'a mut Fib,
    duration: Duration,
}

impl<'a> Future for DurationFuture<'a> {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _cx: &mut Context) -> Poll<Self::Output> {
        match self.fib.state {
            // We assume the executor will only poll it once the duration is over
            State::Halted => {
                self.fib.state = State::Running;
                Poll::Ready(())
            }
            State::Running => {
                self.fib.state = State::Halted;
                self.fib
                    .waiting_on_duration
                    .borrow_mut()
                    .push_back((self.duration, self.fib.id));
                Poll::Pending
            }
        }
    }
}

type Fut = Pin<Box<dyn Future<Output = ()>>>;

pub struct Executor {
    ready: VecDeque<(u32, Fut)>,
    pending: HashMap<u32, Fut>,
    next_id: u32,
    waiting_next_tick: Rc<RefCell<VecDeque<u32>>>,
    waiting_on_duration: Rc<RefCell<VecDeque<(Duration, u32)>>>,
}

impl Executor {
    pub fn new() -> Self {
        Executor {
            ready: VecDeque::new(),
            pending: HashMap::new(),
            next_id: 0,
            waiting_next_tick: Rc::new(RefCell::new(VecDeque::new())),
            waiting_on_duration: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    pub fn push<C, F>(&mut self, closure: C)
    where
        F: Future<Output = ()> + 'static,
        C: FnOnce(Fib) -> F,
    {
        let fib = Fib {
            state: State::Running,
            id: self.next_id,
            waiting_next_tick: Rc::clone(&self.waiting_next_tick),
            waiting_on_duration: Rc::clone(&self.waiting_on_duration),
        };
        self.ready.push_back((self.next_id, Box::pin(closure(fib))));
        self.next_id += 1;
    }

    pub fn run_with_world(&mut self, world: Rc<WorldCell>) {
        let waker = create();
        let mut context = Context::from_waker(&waker);

        let time = world.resource::<Time>();
        let dt = time.delta_seconds();
        let mut on_duration = self.waiting_on_duration.borrow_mut();
        on_duration.iter_mut().for_each(|(duration, id)| {
            duration.advance_by(dt);
            if duration.d <= 0.0 {
                self.ready
                    .push_back((*id, self.pending.remove(id).unwrap()));
            }
        });

        on_duration.retain(|(duration, _)| duration.d > 0.0);
        drop(on_duration);

        let mut next_tick = self.waiting_next_tick.borrow_mut();
        while let Some(id) = next_tick.pop_front() {
            self.ready
                .push_back((id, self.pending.remove(&id).unwrap()));
        }
        drop(next_tick);

        while let Some((id, mut fib)) = self.ready.pop_front() {
            match fib.as_mut().poll(&mut context) {
                Poll::Pending => {
                    self.pending.insert(id, fib);
                }
                Poll::Ready(()) => {}
            }
        }
    }

    pub fn run(&mut self, time: &Time) {
        let waker = create();
        let mut context = Context::from_waker(&waker);

        {
            let dt = time.delta_seconds();
            let mut on_duration = self.waiting_on_duration.borrow_mut();
            on_duration.iter_mut().for_each(|(duration, id)| {
                duration.advance_by(dt);
                if duration.d <= 0.0 {
                    self.ready
                        .push_back((*id, self.pending.remove(id).unwrap()));
                }
            });

            on_duration.retain(|(duration, _)| duration.d > 0.0);
        }

        {
            let mut next_tick = self.waiting_next_tick.borrow_mut();
            while let Some(id) = next_tick.pop_front() {
                self.ready
                    .push_back((id, self.pending.remove(&id).unwrap()));
            }
        }

        while let Some((id, mut fib)) = self.ready.pop_front() {
            match fib.as_mut().poll(&mut context) {
                Poll::Pending => {
                    self.pending.insert(id, fib);
                }
                Poll::Ready(()) => {}
            }
        }
    }
}

pub fn create() -> Waker {
    // Safety: The waker points to a vtable with functions that do nothing. Doing
    // nothing is memory-safe.
    unsafe { Waker::from_raw(RAW_WAKER) }
}

const RAW_WAKER: RawWaker = RawWaker::new(std::ptr::null(), &VTABLE);
const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, w_drop);

unsafe fn clone(_: *const ()) -> RawWaker {
    RAW_WAKER
}
unsafe fn wake(_: *const ()) {}
unsafe fn wake_by_ref(_: *const ()) {}
unsafe fn w_drop(_: *const ()) {}
