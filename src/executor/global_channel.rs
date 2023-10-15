use std::ptr::NonNull;

/// Returns a pair of sender/receiver for a shared mutable queue.
/// The receiver can be cheaply clone, and the channel is dropped
/// once the sender is dropped. There are no synchronization, nor checks,
/// so all operations are marked as unsafe.
pub fn shared_queue<T>() -> (SyncSender<T>, SyncRec<T>) {
    let channel_ptr = Box::into_raw(Box::default());

    let channel_ptr = unsafe { NonNull::new_unchecked(channel_ptr) };
    (SyncSender { channel_ptr }, SyncRec { channel_ptr })
}

impl<T> SyncSender<T> {
    /// Send a value througth the channel.
    /// # Safety
    /// The caller must make sure the channel is still alive, and that
    /// no other concurrent operations are taking place.
    pub unsafe fn send(&self, message: T) {
        let mut channel_ptr = self.channel_ptr;
        let channel = channel_ptr.as_mut();
        channel.push(message);
    }
}

impl<T: 'static> SyncRec<T> {
    /// Consume all values in the channel.
    /// # Safety
    /// The caller must make sure the channel is still alive, and that
    /// no other concurrent operations are taking place.
    pub unsafe fn recv_all(&mut self) -> impl Iterator<Item = T> {
        let mut channel_ptr = self.channel_ptr;
        let channel = channel_ptr.as_mut();
        channel.drain(..)
    }
}

#[derive(Copy)]
pub struct SyncSender<T> {
    channel_ptr: NonNull<Vec<T>>,
}

impl<T> Clone for SyncSender<T> {
    fn clone(&self) -> Self {
        Self {
            channel_ptr: self.channel_ptr,
        }
    }
}

pub struct SyncRec<T> {
    channel_ptr: NonNull<Vec<T>>,
}

impl<T> Drop for SyncRec<T> {
    fn drop(&mut self) {
        // Safety: Only the receiver can drop the channel, and there is only one receiver.
        // hence, the pointer is still valid.
        unsafe { drop(Box::from_raw(self.channel_ptr.as_ptr())) }
    }
}

unsafe impl<T: Send> Send for SyncRec<T> {}
unsafe impl<T: Send> Send for SyncSender<T> {}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    pub fn single_threaded_snd_rec() {
        let (tx, mut rx) = shared_queue();
        unsafe {
            tx.send(1);
            assert_eq!(rx.recv_all().next(), Some(1));
        }
    }
}
