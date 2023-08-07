use std::task::{RawWaker, RawWakerVTable, Waker};

pub fn create() -> Waker {
    // Safety: The waker points to a vtable with functions that do nothing. Doing
    // nothing is memory-safe.
    unsafe { Waker::from_raw(RAW_WAKER) }
}

const RAW_WAKER: RawWaker = RawWaker::new(std::ptr::null(), &VTABLE);
const VTABLE: RawWakerVTable = RawWakerVTable::new(clone, do_nothing, do_nothing, do_nothing);

unsafe fn clone(_: *const ()) -> RawWaker {
    RAW_WAKER
}
unsafe fn do_nothing(_: *const ()) {}
