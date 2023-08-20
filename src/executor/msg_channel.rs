use std::{cell::Cell, rc::Rc};

// A simple single receiver, multiple sender channel (will probably change later)
// with a capacity of 1 (Mostly here for code clarity)

pub(crate) struct Receiver<T> {
    channel: Rc<Cell<Option<T>>>,
}

impl<T> Default for Receiver<T> {
    fn default() -> Self {
        Self::new()
    }
}

pub(crate) struct Sender<T> {
    channel: Rc<Cell<Option<T>>>,
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            channel: Rc::clone(&self.channel),
        }
    }
}

impl<T> Receiver<T> {
    pub(crate) fn new() -> Self {
        Self {
            channel: Rc::new(Cell::new(None)),
        }
    }

    pub(crate) fn receive(&self) -> Option<T> {
        self.channel.replace(None)
    }

    pub(crate) fn sender(&self) -> Sender<T> {
        Sender {
            channel: Rc::clone(&self.channel),
        }
    }
}

impl<T> Sender<T> {
    pub(crate) fn send(&self, val: T) {
        self.channel.replace(Some(val));
    }
}
