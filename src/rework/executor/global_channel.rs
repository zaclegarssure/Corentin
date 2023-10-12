use std::sync::mpsc::{self, Receiver as AsyncRec};
use std::{collections::VecDeque, ptr::NonNull, sync::mpsc::Sender as AsyncSender};

/// Create a pair of sender/receiver, for a hybrid mpsc channel. The channel is "hybrid" in the
/// sense that it can be used to communicate either via atomics operation, or using a simple shared
/// mutable queue.
pub fn global_channel<T>() -> (GlobalSender<T>, GlobalReceiver<T>) {
    let (tx, rx) = mpsc::channel();
    let (t, r) = sync_channel();
    (
        GlobalSender {
            single: t,
            multi: tx,
        },
        GlobalReceiver {
            single: r,
            multi: rx,
        },
    )
}

fn sync_channel<T>() -> (SyncSender<T>, SyncRec<T>) {
    let channel_ptr = Box::into_raw(Box::new(GlobalQueue::new()));

    let channel_ptr = unsafe { NonNull::new_unchecked(channel_ptr) };
    (SyncSender { channel_ptr }, SyncRec { channel_ptr })
}

pub struct GlobalSender<T> {
    single: SyncSender<T>,
    multi: AsyncSender<T>,
}

impl<T> Clone for GlobalSender<T> {
    fn clone(&self) -> Self {
        Self {
            single: self.single.clone(),
            multi: self.multi.clone(),
        }
    }
}

impl<T> GlobalSender<T> {
    pub unsafe fn send_sync(&self, message: T) {
        let mut channel_ptr = self.single.channel_ptr;
        let channel = unsafe { channel_ptr.as_mut() };
        channel.queue.push_back(message);
    }

    pub fn send(&self, message: T) {
        let _ = self.multi.send(message);
    }
}

pub struct GlobalReceiver<T> {
    single: SyncRec<T>,
    multi: AsyncRec<T>,
}

impl<T> GlobalReceiver<T> {
    pub unsafe fn try_recv_sync(&self) -> Option<T> {
        let mut channel_ptr = self.single.channel_ptr;
        let channel = unsafe { channel_ptr.as_mut() };
        channel.queue.pop_front()
    }

    pub fn try_recv(&self) -> Option<T> {
        let mut channel_ptr = self.single.channel_ptr;
        let channel = unsafe { channel_ptr.as_mut() };
        match channel.queue.pop_front() {
            Some(val) => Some(val),
            None => match self.multi.try_recv() {
                Ok(val) => Some(val),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                _ => unreachable!(),
            },
        }
    }
}

#[derive(Copy)]
struct SyncSender<T> {
    channel_ptr: NonNull<GlobalQueue<T>>,
}

impl<T> Clone for SyncSender<T> {
    fn clone(&self) -> Self {
        Self {
            channel_ptr: self.channel_ptr,
        }
    }
}

impl<T> Drop for SyncRec<T> {
    fn drop(&mut self) {
        // Safety: blablabla
        unsafe { drop(Box::from_raw(self.channel_ptr.as_ptr())) }
    }
}

unsafe impl<T: Send> Send for GlobalSender<T> {}
unsafe impl<T: Send> Send for GlobalReceiver<T> {}

struct SyncRec<T> {
    channel_ptr: NonNull<GlobalQueue<T>>,
}

struct GlobalQueue<T> {
    queue: VecDeque<T>,
}

impl<T> GlobalQueue<T> {
    fn new() -> Self {
        Self {
            queue: VecDeque::new(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    pub fn single_threaded_snd_rec() {
        let (tx, rx) = global_channel();
        unsafe {
            tx.send_sync(1);
            assert_eq!(rx.try_recv_sync(), Some(1));
        }
    }
}
