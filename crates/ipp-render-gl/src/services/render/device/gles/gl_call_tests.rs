//! Exact-signature physical callbacks exercise the real retained-state helpers.
use super::*;
use std::cell::RefCell;

thread_local! { static CALLS: RefCell<Vec<&'static str>> = const { RefCell::new(Vec::new()) }; }

unsafe extern "system" fn string(name: u32) -> *const u8 {
    if name == 0x1F02 {
        c"OpenGL ES 3.0 test".as_ptr().cast()
    } else {
        c"".as_ptr().cast()
    }
}

unsafe extern "system" fn integer(name: u32, result: *mut i32) {
    // SAFETY: Constructor supplies one scalar or the two-element viewport limit;
    // this exact-signature callback writes only the specified exclusive storage.
    unsafe {
        result.write(if name == 0x8FBB {
            0
        } else {
            64
        });
        if name == 0x0D3A {
            result.add(1).write(64);
        }
    }
}

unsafe extern "system" fn timer_string(name: u32) -> *const u8 {
    if name == 0x1F03 {
        c"GL_EXT_disjoint_timer_query".as_ptr().cast()
    } else {
        // SAFETY: Same exact signature and static strings as the base callback.
        unsafe { string(name) }
    }
}

unsafe extern "system" fn generate(_count: i32, result: *mut u32) {
    thread_local! { static NEXT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) }; }
    let name = NEXT.with(|next| {
        next.set(next.get() + 1);
        next.get()
    });
    // SAFETY: Pool requests exactly one name into an exclusive live scalar.
    unsafe {
        result.write(name);
    }
}

unsafe extern "system" fn delete(_count: i32, _names: *const u32) {}

unsafe extern "system" fn pair(_name: u32, _target: u32) {}

unsafe extern "system" fn end(_target: u32) {}

unsafe extern "system" fn bits(_target: u32, _parameter: u32, result: *mut i32) {
    // SAFETY: Caller owns this exact one-scalar output through the call.
    unsafe {
        result.write(64);
    }
}

unsafe extern "system" fn available(_name: u32, _parameter: u32, result: *mut u32) {
    // SAFETY: Caller owns this exact one-scalar output through the call.
    unsafe {
        result.write(1);
    }
}

unsafe extern "system" fn result(name: u32, _parameter: u32, result: *mut u64) {
    // SAFETY: Caller owns this exact one-scalar output through the call.
    unsafe {
        result.write(u64::from(name) * 100);
    }
}

#[test]
fn optional_timer_and_copied_disjoint_pointer_calls_are_profiler_only() {
    // SAFETY: All selected required/EXT callbacks have exact signatures and static lifetime;
    // each output writes only its declared exclusive scalar.
    let mut device = unsafe {
        GlesRenderDevice::from_loader(|symbol| match symbol {
            value if value == c"glGetString" => timer_string as *const () as *const c_void,
            value if value == c"glGetIntegerv" => integer as *const () as *const c_void,
            value if value == c"glGenQueriesEXT" => generate as *const () as *const c_void,
            value if value == c"glDeleteQueriesEXT" => delete as *const () as *const c_void,
            value if value == c"glQueryCounterEXT" || value == c"glBeginQueryEXT" => {
                pair as *const () as *const c_void
            }
            value if value == c"glEndQueryEXT" => end as *const () as *const c_void,
            value if value == c"glGetQueryivEXT" => bits as *const () as *const c_void,
            value if value == c"glGetQueryObjectuivEXT" => available as *const () as *const c_void,
            value if value == c"glGetQueryObjectui64vEXT" => result as *const () as *const c_void,
            _ => Functions::test_pointer(symbol),
        })
        .unwrap()
    };
    assert!(device.gl_calls_start());
    let token = device.gpu_start().unwrap();
    device.gpu_end(token);
    assert_eq!(
        device.gpu_poll(token),
        super::super::RenderGpuAvailability::Available {
            duration_ns: 100
        }
    );
    let counts = device.gl_calls_snapshot().unwrap();
    assert_eq!(
        (counts.draws, counts.state, counts.uploads, counts.other),
        (0, 0, 0, 0)
    );
    assert_eq!(counts.profiler, 12);
    device.gl_calls_stop();
}

unsafe extern "system" fn program(_program: u32) {
    CALLS.with(|calls| calls.borrow_mut().push("program"));
}

unsafe extern "system" fn vertex_array(_vao: u32) {
    CALLS.with(|calls| calls.borrow_mut().push("vao"));
}

unsafe extern "system" fn framebuffer(_target: u32, _name: u32) {
    CALLS.with(|calls| calls.borrow_mut().push("framebuffer"));
}

fn device() -> GlesRenderDevice {
    // SAFETY: Every callback has its exact declared GL signature and static lifetime.
    // Constructor storage writes obey the GL output lengths; unused callbacks retain nothing.
    unsafe {
        GlesRenderDevice::from_loader(|symbol| match symbol {
            value if value == c"glGetString" => string as *const () as *const c_void,
            value if value == c"glGetIntegerv" => integer as *const () as *const c_void,
            value if value == c"glUseProgram" => program as *const () as *const c_void,
            value if value == c"glBindVertexArray" => vertex_array as *const () as *const c_void,
            value if value == c"glBindFramebuffer" => framebuffer as *const () as *const c_void,
            _ => Functions::test_pointer(symbol),
        })
        .unwrap()
    }
}

#[test]
fn only_issued_calls_increment_and_captures_reset_independently_of_timers() {
    CALLS.with(|calls| calls.borrow_mut().clear());
    let mut device = device();
    assert!(device.gl_calls_start());
    assert_eq!(
        device.gpu_capability(),
        super::super::RenderGpuCapability::Unsupported
    );
    device.use_program(7);
    device.use_program(7);
    device.use_program(8);
    device.bind_vertex_array(3);
    device.bind_vertex_array(3);
    device.bind_framebuffers(2, 2);
    device.bind_framebuffers(2, 2);
    device.bind_framebuffers(3, 2);
    assert_eq!(
        CALLS.with(|calls| calls.borrow().clone()),
        ["program", "program", "vao", "framebuffer", "framebuffer"]
    );
    // SAFETY: Typed static callbacks retain no arguments; null data with zero bytes is valid.
    unsafe {
        device.gl.buffer_data(ARRAY_BUFFER, 0, ptr::null(), 0x88E0);
        device.gl.draw_arrays(TRIANGLES, 0, 3);
        device.gl.get_error();
    }
    let counts = device.gl_calls_snapshot().unwrap();
    assert_eq!(
        (
            counts.draws,
            counts.state,
            counts.uploads,
            counts.other,
            counts.profiler
        ),
        (1, 5, 1, 1, 0)
    );
    device.gl_calls_stop();
    device.use_program(9);
    assert_eq!(device.gl_calls_snapshot(), Some(counts));
    assert!(device.gl_calls_start());
    assert_eq!(device.gl_calls_snapshot(), Some(Default::default()));
    device.use_program(10);
    assert_eq!(device.gl_calls_snapshot().unwrap().state, 1);
    device.gl_calls_stop();
}
