use std::{mem::MaybeUninit, ptr::NonNull};

use oneshot::TryRecvError;
use sync_states::*;
/// This modules contains the definition of one-shot channels, which can be used either
/// concurrently or not.
///
/// The idea is that coroutines should be aware if they are currently running on 1 or multiple
/// thread, and therefore choose the appropriate way to communicate with the rest of the world

pub fn sync_once_channel<T>() -> (OnceSender<T>, OnceRec<T>) {
    let channel_ptr = Box::into_raw(Box::new(SyncChannel::new()));

    let channel_ptr = unsafe { NonNull::new_unchecked(channel_ptr) };
    (OnceSender { channel_ptr }, OnceRec { channel_ptr })
}

pub struct OnceSender<T> {
    channel_ptr: NonNull<SyncChannel<T>>,
}

pub struct OnceRec<T> {
    channel_ptr: NonNull<SyncChannel<T>>,
}

impl<T> OnceSender<T> {
    pub fn send(self, message: T) {
        let mut channel_ptr = self.channel_ptr;

        // Don't run our Drop implementation if send was called, any cleanup now happens here
        std::mem::forget(self);

        // SAFETY: The channel exists on the heap for the entire duration of this method and we
        // only ever acquire shared references to it. Note that if the receiver disconnects it
        // does not free the channel.
        let channel = unsafe { channel_ptr.as_mut() };

        match channel.state {
            INIT => {
                channel.state = DONE;
                channel.message.write(message);
            }
            DROP_REC => {
                // SAFETY: The receiver has been dropped, we can therefore safely drop
                // the channel.
                unsafe { drop(Box::from_raw(channel_ptr.as_ptr())) }
            }
            _ => unreachable!(),
        }
    }

    /// Returns true iff the sender is still listening
    pub fn is_alive(&self) -> bool {
        // SAFETY: The channel exists on the heap for the entire duration of this method and we
        // only ever acquire shared references to it. Note that if the receiver disconnects it
        // does not free the channel.
        let channel = unsafe { self.channel_ptr.as_ref() };

        channel.state != DROP_REC
    }
}

impl<T> OnceRec<T> {
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        let mut channel_ptr = self.channel_ptr;

        // SAFETY: The channel exists on the heap for the entire duration of this method and we
        // only ever acquire shared references to it. Note that if the receiver disconnects it
        // does not free the channel.
        let channel = unsafe { channel_ptr.as_mut() };

        match channel.state {
            INIT => Err(TryRecvError::Empty),
            DROP_SND => Err(TryRecvError::Disconnected),
            DONE => {
                let res = std::mem::replace(&mut channel.message, MaybeUninit::uninit());
                // Safety: We are in the done state, the message must be initialized
                unsafe { Ok(res.assume_init()) }
            }
            _ => unreachable!(),
        }
    }
}

impl<T> Drop for OnceRec<T> {
    fn drop(&mut self) {
        let mut channel_ptr = self.channel_ptr;
        // SAFETY: The channel exists on the heap for the entire duration of this method and we
        // only ever acquire shared references to it. Note that if the receiver disconnects it
        // does not free the channel.
        let channel = unsafe { channel_ptr.as_mut() };
        match channel.state {
            DROP_SND | DONE => {
                // SAFETY: The receiver has been dropped, we can therefore safely drop
                // the channel.
                unsafe { drop(Box::from_raw(channel_ptr.as_ptr())) }
            }
            INIT => {
                channel.state = DROP_REC;
            }
            _ => unreachable!(),
        }
    }
}

impl<T> Drop for OnceSender<T> {
    fn drop(&mut self) {
        let mut channel_ptr = self.channel_ptr;
        // SAFETY: The channel exists on the heap for the entire duration of this method and we
        // only ever acquire shared references to it. Note that if the receiver disconnects it
        // does not free the channel.
        let channel = unsafe { channel_ptr.as_mut() };
        match channel.state {
            DROP_REC => {
                // SAFETY: The receiver has been dropped, we can therefore safely drop
                // the channel.
                unsafe { drop(Box::from_raw(channel_ptr.as_ptr())) }
            }
            INIT => {
                channel.state = DROP_SND;
            }
            _ => unreachable!(),
        }
    }
}

unsafe impl<T: Send> Send for OnceSender<T> {}
unsafe impl<T: Send> Send for OnceRec<T> {}

struct SyncChannel<T> {
    state: u8,
    message: MaybeUninit<T>,
}

impl<T> SyncChannel<T> {
    fn new() -> Self {
        Self {
            state: INIT,
            message: MaybeUninit::uninit(),
        }
    }
}

//pub fn once_channel<T>() -> (Sender<T>, Receiver<T>) {
//    let (tx, rx) = oneshot::channel();
//    let (t, r) = sync_once_channel();
//    (
//        Sender {
//            single: t,
//            multi: tx,
//        },
//        Receiver {
//            single: r,
//            multi: rx,
//        },
//    )
//}
//
//pub struct Sender<T> {
//    single: OnceSender<T>,
//    multi: AsyncSender<T>,
//}

//impl<T> Sender<T> {
//    /// Send a value througth that channel as if we are running in a single threaded
//    /// context.
//    ///
//    /// # Safety
//    /// The caller must ensure that no other thread can concurrently list to the channel
//    pub unsafe fn send_sync(self, message: T) {
//        let mut channel_ptr = self.single.channel_ptr;
//
//        // Don't run our Drop implementation if send was called, any cleanup now happens here
//        std::mem::forget(self);
//
//        // SAFETY: The channel exists on the heap for the entire duration of this method and we
//        // only ever acquire shared references to it. Note that if the receiver disconnects it
//        // does not free the channel.
//        let channel = unsafe { channel_ptr.as_mut() };
//
//        match channel.state {
//            INIT => {
//                channel.state = DONE;
//                channel.message.write(message);
//            }
//            DROP_REC => {
//                // SAFETY: The receiver has been dropped, we can therefore safely drop
//                // the channel.
//                unsafe { drop(Box::from_raw(channel_ptr.as_ptr())) }
//            }
//            _ => unreachable!(),
//        }
//    }
//
//    /// Send a value througth that channel concurrently.
//    pub fn send(self, message: T) {
//        let _ = self.multi.send(message);
//    }
//}

mod sync_states {
    /// The initial channel state. Active while both endpoints are still alive, no message has been
    /// sent, and the receiver is not receiving.
    pub const INIT: u8 = 0b00;
    /// A message has been sent to the channel, but the receiver has not yet read it.
    pub const DONE: u8 = 0b11;
    /// The sender has been dropped
    pub const DROP_SND: u8 = 0b10;
    /// The receiver has been dropped
    pub const DROP_REC: u8 = 0b01;
}

//pub struct Receiver<T> {
//    single: OnceRec<T>,
//    multi: AsyncRec<T>,
//}

//impl<T> Receiver<T> {
//    /// Send a value througth that channel as if we are running in a single threaded
//    /// context.
//    ///
//    /// # Safety
//    /// The caller must ensure that no other thread can concurrently list to the channel
//    pub unsafe fn try_recv_sync(&self) -> Result<T, TryRecvError> {
//        let mut channel_ptr = self.single.channel_ptr;
//
//        // SAFETY: The channel exists on the heap for the entire duration of this method and we
//        // only ever acquire shared references to it. Note that if the receiver disconnects it
//        // does not free the channel.
//        let channel = unsafe { channel_ptr.as_mut() };
//
//        match channel.state {
//            INIT => Err(TryRecvError::Empty),
//            DROP_SND => Err(TryRecvError::Disconnected),
//            DONE => {
//                let res = std::mem::replace(&mut channel.message, MaybeUninit::uninit());
//                // Safety: We are in the done state, the message must be initialized
//                unsafe { Ok(res.assume_init()) }
//            }
//            _ => unreachable!(),
//        }
//    }
//
//    /// Try to receive a value througth that channel concurrently.
//    pub fn try_recv(&self) -> Result<T, TryRecvError> {
//        // Safety: Since we are in a concurrent context, no one is writing to the single threaded
//        // channel. However someone may have written to it previously, so we still check it, just
//        // in case.
//        if let Ok(val) = unsafe { self.try_recv_sync() } {
//            Ok(val)
//        } else {
//            self.multi.try_recv()
//        }
//    }
//}
