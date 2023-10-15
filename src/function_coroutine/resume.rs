use std::ptr::NonNull;

/// A value which is shared on each resumed with a [`Future`].
/// Used to emulate resume arguments to a [`Coroutine`].
///
/// It's really similar to a [`Box`], might even be possible to just use that instead.
pub struct Resume<T> {
    value: NonNull<T>,
}

impl<T> Resume<T> {
    pub fn new(initial: T) -> Self {
        Self {
            value: unsafe { NonNull::new_unchecked(Box::into_raw(Box::new(initial))) },
        }
    }

    /// Get the underlying raw pointer.
    pub fn get_raw(&self) -> NonNull<T> {
        self.value
    }

    /// Set the value.
    /// # Safety
    /// This must not be called while the [`Future`] is using this value.
    pub unsafe fn set(&mut self, value: T) {
        *self.value.as_mut() = value;
    }

    /// Get the value.
    /// # Safety
    /// This must not be called while the [`Future`] is using this value.
    pub unsafe fn get(&self) -> &T {
        self.value.as_ref()
    }

    /// Get the value mutably.
    /// # Safety
    /// This must not be called while the [`Future`] is using this value.
    pub unsafe fn get_mut(&mut self) -> &mut T {
        self.value.as_mut()
    }
}

impl<T> Drop for Resume<T> {
    fn drop(&mut self) {
        unsafe {
            drop(Box::from_raw(self.value.as_ptr()));
        }
    }
}
