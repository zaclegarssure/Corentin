use std::cell::UnsafeCell;

use thread_local::ThreadLocal;

pub struct Channel<T: Send> {
    chan: ThreadLocal<UnsafeCell<Vec<T>>>,
}

impl<T: Send> Default for Channel<T> {
    fn default() -> Self {
        Self {
            chan: Default::default(),
        }
    }
}

impl<T: Send> Channel<T> {
    pub fn send(&self, value: T) {
        let cell = self.chan.get_or_default();
        // Safety: We could use refcell, to be safe actually
        unsafe { cell.get().as_mut().unwrap() }.push(value);
    }

    pub fn receive(&mut self) -> Vec<T> {
        self.chan
            .iter_mut()
            .flat_map(|q| q.get_mut().drain(..))
            .collect()
    }
}
