//! Example-only GLES interception. Production devices and shaders are unchanged.
use super::Result;
use std::cell::Cell;
use std::ffi::{CStr, c_void};
use std::time::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Mode {
    #[default]
    Normal,
    NoDraws,
    Quarter,
    Half,
    Discard,
    NoSetup,
}

impl Mode {
    pub fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::NoDraws => "no-draws",
            Self::Quarter => "quarter-draws",
            Self::Half => "half-draws",
            Self::Discard => "rasterizer-discard",
            Self::NoSetup => "no-draws-or-setup",
        }
    }
}

#[derive(Clone, Copy, Default)]
struct Control {
    enabled: bool,
    timing: bool,
    mode: Mode,
}

#[derive(Clone, Copy, Default)]
pub(super) struct Metric {
    pub calls: u64,
    pub ns: u64,
}

#[derive(Clone, Copy)]
enum Group {
    Draw,
    Setup,
    Pass,
    Present,
}

thread_local! {
    static CONTROL: Cell<Control> = Cell::new(Control::default());
    static ATTEMPTED: Cell<u64> = const { Cell::new(0) };
    static SUBMITTED: Cell<u64> = const { Cell::new(0) };
    static METRICS: [Cell<Metric>; 48] = const { [const { Cell::new(Metric { calls: 0, ns: 0 }) }; 48] };
}

pub(super) fn start(mode: Mode, timing: bool) {
    ATTEMPTED.set(0);
    SUBMITTED.set(0);
    METRICS.with(|metrics| {
        metrics
            .iter()
            .for_each(|metric| metric.set(Metric::default()))
    });
    CONTROL.set(Control {
        enabled: true,
        timing,
        mode,
    });
}

pub(super) fn stop() -> (u64, u64, [Metric; 48]) {
    CONTROL.set(Control::default());
    (
        ATTEMPTED.get(),
        SUBMITTED.get(),
        METRICS.with(|metrics| std::array::from_fn(|i| metrics[i].get())),
    )
}

fn skip(control: Control, group: Group) -> bool {
    if !control.enabled {
        return false;
    }
    match group {
        Group::Draw => {
            let ordinal = ATTEMPTED.get();
            ATTEMPTED.set(ordinal + 1);
            let submit = match control.mode {
                Mode::Normal | Mode::Discard => true,
                Mode::NoDraws | Mode::NoSetup => false,
                Mode::Quarter => ordinal.is_multiple_of(4),
                Mode::Half => ordinal.is_multiple_of(2),
            };
            SUBMITTED.set(SUBMITTED.get() + u64::from(submit));
            !submit
        }
        Group::Setup | Group::Present => control.mode == Mode::NoSetup,
        Group::Pass => false,
    }
}

macro_rules! hooks {
    ($($id:literal $name:ident $symbol:literal ($($arg:ident: $ty:ty),*) -> $result:ty, $group:ident;)*) => {
        $(mod $name {
            use super::*;

            type Function = unsafe extern "system" fn($($ty),*) -> $result;

            thread_local! {
                pub(super) static ORIGINAL: Cell<Option<Function>> = const { Cell::new(None) };
            }

            pub(super) unsafe extern "system" fn call($($arg: $ty),*) -> $result {
                let control = CONTROL.get();
                if skip(control, Group::$group) {
                    return Default::default();
                }
                if matches!(Group::$group, Group::Present) && control.mode == Mode::Discard {
                    // SAFETY: The current thread's retained Context owns these entry
                    // points. Restore rasterization for the fullscreen presentation;
                    // scalar-only calls retain no pointers or aliases.
                    unsafe { disable::ORIGINAL.get().unwrap()(0x8C89) };
                }
                let started = control.timing.then(Instant::now);
                // SAFETY: The loader installs the exact GLES signature on the same
                // thread before use. Context outlives the device and all callbacks.
                // Arguments preserve the production call's lifetimes and aliasing;
                // no wrapper retains caller pointers or modifies pointed-to data.
                let result = unsafe { ORIGINAL.get().unwrap()($($arg),*) };
                if let Some(started) = started {
                    let ns = started.elapsed().as_nanos() as u64;
                    METRICS.with(|metrics| {
                        let mut value = metrics[$id].get();
                        value.calls += 1;
                        value.ns += ns;
                        metrics[$id].set(value);
                    });
                }
                result
            }
        })*

        fn intercept(context: &crate::egl::Context, name: &CStr) -> *const c_void {
            let address = context.gl(name);
            if address.is_null() {
                return address;
            }
            match name.to_bytes() {
                $($symbol => {
                    // SAFETY: Literal GLES symbol and exact signature correspond.
                    // Context stays current/alive through all device use; original
                    // addresses remain thread-local and are replaced before reuse.
                    $name::ORIGINAL.set(Some(unsafe { std::mem::transmute::<
                        *const c_void, unsafe extern "system" fn($($ty),*) -> $result
                    >(address) }));
                    $name::call as *const c_void
                },)*
                b"glDisable" => {
                    // SAFETY: Exact GLES glDisable signature; same Context lifetime
                    // and single-thread ownership as the other intercepted entries.
                    disable::ORIGINAL.set(Some(unsafe { std::mem::transmute::<
                        *const c_void, unsafe extern "system" fn(u32)
                    >(address) }));
                    disable::call as *const c_void
                }
                _ => address,
            }
        }

        pub(super) fn names() -> &'static [(usize, &'static str)] {
            &[$(($id, stringify!($name)),)*]
        }
    };
}

mod disable {
    use super::*;

    thread_local! {
        pub(super) static ORIGINAL: Cell<Option<unsafe extern "system" fn(u32)>> = const { Cell::new(None) };
    }

    pub(super) unsafe extern "system" fn call(capability: u32) {
        let control = CONTROL.get();
        // SAFETY: Context owns these exact GLES entries on this thread and remains
        // live through callbacks. Scalar state updates retain no CPU pointers.
        unsafe {
            if capability == 0x8C89 && control.enabled && control.mode == Mode::Discard {
                enable::ORIGINAL.get().unwrap()(capability);
            } else {
                ORIGINAL.get().unwrap()(capability);
            }
        }
    }
}

hooks! {
    0 draw_elements b"glDrawElements"(mode: u32, count: i32, kind: u32, offset: *const c_void) -> (), Draw;
    1 draw_instances b"glDrawElementsInstanced"(mode: u32, count: i32, kind: u32, offset: *const c_void, instances: i32) -> (), Draw;
    2 draw_arrays b"glDrawArrays"(mode: u32, first: i32, count: i32) -> (), Present;
    3 uniform_matrix b"glUniformMatrix4fv"(location: i32, count: i32, transpose: u8, value: *const f32) -> (), Setup;
    4 uniform_rgb b"glUniform3fv"(location: i32, count: i32, value: *const f32) -> (), Setup;
    5 uniform_vec4 b"glUniform4fv"(location: i32, count: i32, value: *const f32) -> (), Setup;
    6 uniform_float b"glUniform1f"(location: i32, value: f32) -> (), Setup;
    7 uniform_int b"glUniform1i"(location: i32, value: i32) -> (), Setup;
    8 use_program b"glUseProgram"(program: u32) -> (), Setup;
    9 bind_vertex_array b"glBindVertexArray"(vao: u32) -> (), Setup;
    10 attrib_rgb b"glVertexAttrib3f"(slot: u32, x: f32, y: f32, z: f32) -> (), Setup;
    11 active_texture b"glActiveTexture"(unit: u32) -> (), Setup;
    12 bind_texture b"glBindTexture"(target: u32, texture: u32) -> (), Setup;
    13 bind_sampler b"glBindSampler"(unit: u32, sampler: u32) -> (), Setup;
    14 bind_buffer b"glBindBuffer"(target: u32, buffer: u32) -> (), Setup;
    15 buffer_sub_data b"glBufferSubData"(target: u32, offset: isize, size: isize, data: *const c_void) -> (), Setup;
    16 bind_buffer_base b"glBindBufferBase"(target: u32, index: u32, buffer: u32) -> (), Setup;
    17 enable_attrib b"glEnableVertexAttribArray"(index: u32) -> (), Setup;
    18 disable_attrib b"glDisableVertexAttribArray"(index: u32) -> (), Setup;
    19 attrib_pointer b"glVertexAttribPointer"(index: u32, size: i32, kind: u32, normalized: u8, stride: i32, offset: *const c_void) -> (), Setup;
    20 attrib_divisor b"glVertexAttribDivisor"(index: u32, divisor: u32) -> (), Setup;
    21 get_error b"glGetError"() -> u32, Pass;
    22 get_integer b"glGetIntegerv"(name: u32, data: *mut i32) -> (), Pass;
    23 bind_framebuffer b"glBindFramebuffer"(target: u32, framebuffer: u32) -> (), Pass;
    24 viewport b"glViewport"(x: i32, y: i32, width: i32, height: i32) -> (), Pass;
    25 enable b"glEnable"(capability: u32) -> (), Pass;
    26 clear b"glClear"(mask: u32) -> (), Pass;
    27 depth_mask b"glDepthMask"(value: u8) -> (), Pass;
    28 color_mask b"glColorMask"(r: u8, g: u8, b: u8, a: u8) -> (), Pass;
    29 blend_func b"glBlendFuncSeparate"(sr: u32, dr: u32, sa: u32, da: u32) -> (), Pass;
    30 blend_equation b"glBlendEquation"(mode: u32) -> (), Pass;
}

pub(super) fn device(context: &crate::egl::Context) -> Result<ipp_render_gl::GlesRenderDevice> {
    // SAFETY: The profiling Host retains its current Context until after all
    // renderer/provider resources drop. Interception preserves exact signatures,
    // argument lifetimes and single-thread ownership; no CPU pointer is retained.
    Ok(unsafe { ipp_render_gl::GlesRenderDevice::from_loader(|name| intercept(context, name))? })
}
