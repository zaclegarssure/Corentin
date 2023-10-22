use std::{cell::UnsafeCell, sync::Arc};

/// A value which is shared on each resumed with a [`Future`].
/// Used to emulate resume arguments to a [`Coroutine`].
pub struct Resume<T> {
    value: Arc<UnsafeCell<T>>,
}

impl<T> Clone for Resume<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
        }
    }
}

impl<T> Resume<T> {
    pub fn new(initial: T) -> Self {
        Self {
            value: Arc::new(UnsafeCell::new(initial)),
        }
    }

    /// Set the value.
    /// # Safety
    /// This must not be called while the [`Future`] is using this value.
    pub unsafe fn set(&mut self, value: T) {
        *self.value.as_ref().get() = value;
    }

    /// Get the value.
    /// # Safety
    /// This must not be called while the [`Future`] is using this value.
    pub unsafe fn get(&self) -> &T {
        &*self.value.as_ref().get()
    }

    /// Get the value mutably.
    /// # Safety
    /// This must not be called while the [`Future`] is using this value.
    pub unsafe fn get_mut(&mut self) -> &mut T {
        &mut *self.value.as_ref().get()
    }

    pub fn scope_droped(&self) -> bool {
        Arc::strong_count(&self.value) == 1
    }
}
