use super::super::{GlesRenderDevice, loader::Functions};
use crate::{RenderDevice, RenderError};
use std::{cell::RefCell, ffi::c_void};

#[derive(Default)]
struct Calls {
    flushes: usize,
    deletes: usize,
    polls: Vec<(u32, u64)>,
    status: u32,
    error: u32,
    null_fence: bool,
}

thread_local! { static CALLS: RefCell<Calls> = RefCell::new(Calls::default()); }

unsafe extern "system" fn string(name: u32) -> *const u8 {
    if name == 0x1F02 {
        c"OpenGL ES 3.0 test".as_ptr().cast()
    } else {
        c"".as_ptr().cast()
    }
}

unsafe extern "system" fn integer(name: u32, output: *mut i32) {
    // SAFETY: Constructor supplies exclusive scalar/two-value output storage;
    // exact-signature callback writes precisely the queried GL count.
    unsafe {
        output.write(if name == 0x8FBB {
            0
        } else {
            64
        });
        if name == 0x0D3A {
            output.add(1).write(64);
        }
    }
}

unsafe extern "system" fn fence(condition: u32, flags: u32) -> *mut c_void {
    assert_eq!((condition, flags), (0x9117, 0));
    if CALLS.with(|calls| calls.borrow().null_fence) {
        std::ptr::null_mut()
    } else {
        std::ptr::dangling_mut::<c_void>()
    }
}

unsafe extern "system" fn flush() {
    CALLS.with(|calls| calls.borrow_mut().flushes += 1);
}

unsafe extern "system" fn delete(_fence: *mut c_void) {
    CALLS.with(|calls| calls.borrow_mut().deletes += 1);
}

unsafe extern "system" fn poll(_fence: *mut c_void, flags: u32, timeout: u64) -> u32 {
    CALLS.with(|calls| {
        let mut calls = calls.borrow_mut();
        calls.polls.push((flags, timeout));
        calls.status
    })
}

unsafe extern "system" fn error() -> u32 {
    CALLS.with(|calls| std::mem::take(&mut calls.borrow_mut().error))
}

fn device() -> GlesRenderDevice {
    // SAFETY: Static exact-signature stubs retain/dereference no GL handles;
    // queries above obey required output sizes, and all remaining calls are inert.
    unsafe {
        GlesRenderDevice::from_loader(|symbol| match symbol {
            value if value == c"glGetString" => string as *const () as *const c_void,
            value if value == c"glGetIntegerv" => integer as *const () as *const c_void,
            value if value == c"glFenceSync" => fence as *const () as *const c_void,
            value if value == c"glFlush" => flush as *const () as *const c_void,
            value if value == c"glDeleteSync" => delete as *const () as *const c_void,
            value if value == c"glClientWaitSync" => poll as *const () as *const c_void,
            value if value == c"glGetError" => error as *const () as *const c_void,
            _ => Functions::test_pointer(symbol),
        })
        .unwrap()
    }
}

#[test]
fn polling_never_waits_and_flushes_progress_without_another_frame() {
    CALLS.with(|calls| *calls.borrow_mut() = Calls::default());
    let mut device = device();
    let fence = device.create_completion_fence().unwrap();
    assert_eq!(CALLS.with(|calls| calls.borrow().flushes), 1);
    for (status, expected) in [(0x911B, false), (0x911A, true), (0x911C, true)] {
        CALLS.with(|calls| calls.borrow_mut().status = status);
        assert_eq!(device.poll_completion_fence(fence).unwrap(), expected);
    }
    CALLS.with(|calls| calls.borrow_mut().status = 0x911D);
    assert!(device.poll_completion_fence(fence).is_err());
    assert_eq!(
        CALLS.with(|calls| calls.borrow().polls.clone()),
        [(0, 0); 4]
    );
    device.delete_completion_fence(fence);
    device.delete_completion_fence(fence);
    assert_eq!(CALLS.with(|calls| calls.borrow().deletes), 1);
    assert!(device.poll_completion_fence(fence).is_err());
}

#[test]
fn released_and_foreign_ids_never_alias_new_fences() {
    CALLS.with(|calls| *calls.borrow_mut() = Calls::default());
    let mut first = device();
    let old = first.create_completion_fence().unwrap();
    first.delete_completion_fence(old);
    let new = first.create_completion_fence().unwrap();
    assert_ne!(old, new);
    let mut second = device();
    let foreign = second.create_completion_fence().unwrap();
    assert_ne!(new, foreign);
    assert!(first.poll_completion_fence(foreign).is_err());
    first.delete_completion_fence(foreign);
    assert_eq!(CALLS.with(|calls| calls.borrow().deletes), 1);
    drop(first);
    drop(second);
    assert_eq!(CALLS.with(|calls| calls.borrow().deletes), 3);
}

#[test]
fn context_loss_invalidates_all_fences_without_deleting_dead_context_handles() {
    CALLS.with(|calls| *calls.borrow_mut() = Calls::default());
    let mut device = device();
    let first = device.create_completion_fence().unwrap();
    let second = device.create_completion_fence().unwrap();
    CALLS.with(|calls| calls.borrow_mut().error = 0x0507);
    assert!(matches!(
        device.poll_completion_fence(first),
        Err(RenderError::ContextLost)
    ));
    assert!(device.poll_completion_fence(second).is_err());
    device.delete_completion_fence(first);
    drop(device);
    assert_eq!(CALLS.with(|calls| calls.borrow().deletes), 0);
}

#[test]
fn failed_allocation_has_no_published_handle_or_leaked_sync() {
    CALLS.with(|calls| *calls.borrow_mut() = Calls::default());
    let mut device = device();
    CALLS.with(|calls| calls.borrow_mut().null_fence = true);
    assert!(device.create_completion_fence().is_err());
    CALLS.with(|calls| {
        let mut calls = calls.borrow_mut();
        calls.null_fence = false;
        calls.error = 0x0505;
    });
    assert!(device.create_completion_fence().is_err());
    assert_eq!(CALLS.with(|calls| calls.borrow().deletes), 1);
    drop(device);
    assert_eq!(CALLS.with(|calls| calls.borrow().deletes), 1);
}
