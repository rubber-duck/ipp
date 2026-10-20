//! Real-context sentinels catch state leaks immediately, before another draw repairs them.
use crate::egl;
use ipp_render_gl::{GlesDmaBufImage, GlesDmaBufImport, GlesRenderDevice, RenderDevice};
use std::{
    cell::RefCell,
    ffi::{CStr, c_void},
    fs::File,
    os::fd::AsFd,
    time::{Duration, Instant},
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[derive(Default)]
struct ImportCalls {
    framebuffer: Option<unsafe extern "system" fn(u32) -> u32>,
    finish: Option<unsafe extern "system" fn()>,
    fail_framebuffer: bool,
    finishes: u64,
}

thread_local! { static IMPORT_CALLS: RefCell<ImportCalls> = RefCell::new(ImportCalls::default()); }

unsafe extern "system" fn framebuffer_status(target: u32) -> u32 {
    let (real, fail) = IMPORT_CALLS.with(|calls| {
        let calls = calls.borrow();
        (calls.framebuffer, calls.fail_framebuffer)
    });
    if fail {
        return 0x8CD6;
    }
    match real {
        // SAFETY: The test loader stored the current context's exact-signature
        // entrypoint, whose library remains live until after the device drops.
        Some(real) => unsafe { real(target) },
        None => 0,
    }
}

unsafe extern "system" fn finish() {
    let real = IMPORT_CALLS.with(|calls| {
        let mut calls = calls.borrow_mut();
        calls.finishes += 1;
        calls.finish
    });
    if let Some(real) = real {
        // SAFETY: Exact current-context entrypoint cached by the test loader;
        // the real wait executes synchronously before reporting its observation.
        unsafe { real() };
    }
}

pub fn device(context: &egl::Context) -> Result<GlesRenderDevice> {
    let framebuffer = context.gl(c"glCheckFramebufferStatus");
    let finish_pointer = context.gl(c"glFinish");
    if framebuffer.is_null() || finish_pointer.is_null() {
        return Err("missing GLES import test entrypoints".into());
    }
    // SAFETY: These names have the exact GLES signatures; the context owns their
    // library lifetime, and every device operation stays on its current thread.
    unsafe {
        IMPORT_CALLS.with(|calls| {
            *calls.borrow_mut() = ImportCalls {
                framebuffer: Some(std::mem::transmute::<
                    *const c_void,
                    unsafe extern "system" fn(u32) -> u32,
                >(framebuffer)),
                finish: Some(std::mem::transmute::<
                    *const c_void,
                    unsafe extern "system" fn(),
                >(finish_pointer)),
                ..Default::default()
            }
        });
        Ok(GlesRenderDevice::from_loader(|symbol| match symbol {
            value if value == c"glCheckFramebufferStatus" => {
                framebuffer_status as *const () as *const c_void
            }
            value if value == c"glFinish" => finish as *const () as *const c_void,
            _ => context.gl(symbol),
        })?)
    }
}

macro_rules! functions {
    ($($name:ident : $symbol:literal ($($arg:ty),*) -> $ret:ty;)*) => {
        struct Gl {
            $($name: unsafe extern "system" fn($($arg),*) -> $ret,)*
        }

        impl Gl {
            fn load(context: &egl::Context) -> Result<Self> {
                Ok(Self {
                    $($name: {
                        let pointer = context.gl($symbol);
                        if pointer.is_null() {
                            return Err(format!("missing {:?}", $symbol).into());
                        }

                        // SAFETY: Names/signatures match GLES; context libraries stay
                        // live through the test and every call runs on this thread.
                        unsafe {
                            std::mem::transmute::<*const c_void, unsafe extern "system" fn($($arg),*) -> $ret>(pointer)
                        }
                    },)*
                })
            }
        }
    };
}

functions! {
    get: c"glGetIntegerv"(u32, *mut i32) -> ();
    enabled: c"glIsEnabled"(u32) -> u8;
    string: c"glGetString"(u32) -> *const u8;
    active: c"glActiveTexture"(u32) -> ();
    texture: c"glBindTexture"(u32, u32) -> ();
    sampler: c"glBindSampler"(u32, u32) -> ();
    framebuffer: c"glBindFramebuffer"(u32, u32) -> ();
    program: c"glUseProgram"(u32) -> ();
    vao: c"glBindVertexArray"(u32) -> ();
    viewport: c"glViewport"(i32, i32, i32, i32) -> ();
    mask: c"glColorMask"(u8, u8, u8, u8) -> ();
    enable: c"glEnable"(u32) -> ();
    disable: c"glDisable"(u32) -> ();
    gen_textures: c"glGenTextures"(i32, *mut u32) -> ();
    gen_samplers: c"glGenSamplers"(i32, *mut u32) -> ();
    gen_framebuffers: c"glGenFramebuffers"(i32, *mut u32) -> ();
    gen_vaos: c"glGenVertexArrays"(i32, *mut u32) -> ();
    delete_textures: c"glDeleteTextures"(i32, *const u32) -> ();
    delete_samplers: c"glDeleteSamplers"(i32, *const u32) -> ();
    delete_framebuffers: c"glDeleteFramebuffers"(i32, *const u32) -> ();
    delete_vaos: c"glDeleteVertexArrays"(i32, *const u32) -> ();
    error: c"glGetError"() -> u32;
}

const CAPS: [u32; 8] = [
    0x0BE2, 0x0B71, 0x0C11, 0x0B44, 0x0B90, 0x8C89, 0x0BD0, 0x809E,
];

#[derive(Debug, PartialEq)]
struct Snapshot {
    bindings: [i32; 4],
    viewport: [i32; 4],
    mask: [i32; 4],
    units: [[i32; 2]; 2],
    active: i32,
    enabled: [bool; 8],
    srgb: Option<bool>,
}

impl Snapshot {
    fn read(gl: &Gl) -> Self {
        let mut result = Self {
            bindings: [0; 4],
            viewport: [0; 4],
            mask: [0; 4],
            units: [[0; 2]; 2],
            active: 0,
            enabled: [false; 8],
            srgb: None,
        };
        // SAFETY: Queries write their scalar/four-element outputs into exclusive
        // arrays. The context is live; all temporarily switched units are restored.
        unsafe {
            for (name, value) in [0x8CA6, 0x8CAA, 0x8B8D, 0x85B5]
                .into_iter()
                .zip(&mut result.bindings)
            {
                (gl.get)(name, value);
            }
            (gl.get)(0x0BA2, result.viewport.as_mut_ptr());
            (gl.get)(0x0C23, result.mask.as_mut_ptr());
            (gl.get)(0x84E0, &mut result.active);
            for (unit, values) in [0, 3].into_iter().zip(&mut result.units) {
                (gl.active)(0x84C0 + unit);
                (gl.get)(0x8069, &mut values[0]);
                (gl.get)(0x8919, &mut values[1]);
            }
            (gl.active)(result.active as u32);
            result.enabled = CAPS.map(|cap| (gl.enabled)(cap) != 0);
            let extensions = (gl.string)(0x1F03);
            if !extensions.is_null()
                && CStr::from_ptr(extensions.cast())
                    .to_string_lossy()
                    .split_whitespace()
                    .any(|name| name == "GL_EXT_sRGB_write_control")
            {
                result.srgb = Some((gl.enabled)(0x8DB9) != 0);
            }
        }
        result
    }

    fn restore(&self, gl: &Gl) {
        // SAFETY: Snapshot objects remain live for this scope, and all restored
        // values came from this same context; no borrowed CPU pointers escape.
        unsafe {
            (gl.framebuffer)(0x8CA9, self.bindings[0] as u32);
            (gl.framebuffer)(0x8CA8, self.bindings[1] as u32);
            (gl.program)(self.bindings[2] as u32);
            (gl.vao)(self.bindings[3] as u32);
            (gl.viewport)(
                self.viewport[0],
                self.viewport[1],
                self.viewport[2],
                self.viewport[3],
            );
            (gl.mask)(
                self.mask[0] as u8,
                self.mask[1] as u8,
                self.mask[2] as u8,
                self.mask[3] as u8,
            );
            for (unit, values) in [0, 3].into_iter().zip(self.units) {
                (gl.active)(0x84C0 + unit);
                (gl.texture)(0x0DE1, values[0] as u32);
                (gl.sampler)(unit, values[1] as u32);
            }
            (gl.active)(self.active as u32);
            for (cap, enabled) in CAPS
                .into_iter()
                .zip(self.enabled)
                .chain(self.srgb.map(|value| (0x8DB9, value)))
            {
                if enabled {
                    (gl.enable)(cap);
                } else {
                    (gl.disable)(cap);
                }
            }
        }
    }
}

pub fn copy(
    context: &egl::Context,
    device: &mut GlesRenderDevice,
    importer: &GlesDmaBufImport,
    image: &GlesDmaBufImage<'_>,
    destination: &u32,
) -> Result<()> {
    let gl = Gl::load(context)?;
    let original = Snapshot::read(&gl);
    let mut textures = [0; 2];
    let mut samplers = [0; 2];
    let mut framebuffers = [0; 2];
    let mut vao = 0;
    // SAFETY: Current context; new names are owned within this function, outputs
    // are exact-length arrays, and both units/framebuffers exist in GLES3.
    unsafe {
        (gl.gen_textures)(2, textures.as_mut_ptr());
        (gl.gen_samplers)(2, samplers.as_mut_ptr());
        (gl.gen_framebuffers)(2, framebuffers.as_mut_ptr());
        (gl.gen_vaos)(1, &mut vao);
        for (i, unit) in [0, 3].into_iter().enumerate() {
            (gl.active)(0x84C0 + unit);
            (gl.texture)(0x0DE1, textures[i]);
            (gl.sampler)(unit, samplers[i]);
        }
        (gl.framebuffer)(0x8CA9, framebuffers[0]);
        (gl.framebuffer)(0x8CA8, framebuffers[1]);
        (gl.vao)(vao);
        (gl.viewport)(5, 7, 11, 13);
        (gl.mask)(0, 1, 0, 1);
        for cap in CAPS {
            (gl.enable)(cap);
        }
        if original.srgb.is_some() {
            (gl.disable)(0x8DB9);
        }
    }
    let expected = Snapshot::read(&gl);
    let verify = |label: &str| -> Result<()> {
        let actual = Snapshot::read(&gl);
        if actual != expected {
            return Err(format!(
                "{label} leaked GL state: expected {expected:?}, actual {actual:?}"
            )
            .into());
        }
        // SAFETY: Current-context error query has no arguments or output pointers.
        let error = unsafe { (gl.error)() };
        if error != 0 {
            return Err(format!("{label} GL error {error:#x}").into());
        }
        Ok(())
    };
    let result = (|| -> Result<()> {
        verify("sentinel setup")?;
        let unsupported = GlesDmaBufImage {
            modifier: 1,
            ..*image
        };
        if device
            .copy_dma_buf(importer, &unsupported, destination)
            .is_ok()
        {
            return Err("unsupported modifier accepted".into());
        }
        verify("rejected metadata")?;
        if device.copy_dma_buf(importer, image, &0).is_ok() {
            return Err("invalid destination accepted".into());
        }
        verify("rejected destination")?;
        let invalid = File::open("/dev/null")?;
        let planes = [ipp_render_gl::GlesDmaBufPlane {
            fd: invalid.as_fd(),
            ..image.planes[0]
        }];
        let invalid_image = GlesDmaBufImage {
            planes: &planes,
            ..*image
        };
        if device
            .copy_dma_buf(importer, &invalid_image, destination)
            .is_ok()
        {
            return Err("non-DMA-BUF descriptor accepted".into());
        }
        verify("failed EGL import")?;

        let finishes = IMPORT_CALLS.with(|calls| {
            let mut calls = calls.borrow_mut();
            calls.fail_framebuffer = true;
            calls.finishes
        });
        let failed = device.copy_dma_buf(importer, image, destination);
        IMPORT_CALLS.with(|calls| calls.borrow_mut().fail_framebuffer = false);
        if failed.is_ok() {
            return Err("injected incomplete framebuffer accepted".into());
        }
        verify("failed framebuffer after GL state changes")?;
        if IMPORT_CALLS.with(|calls| calls.borrow().finishes) != finishes + 1 {
            return Err("failed import did not complete producer reads".into());
        }
        device.copy_dma_buf(importer, image, destination)?;
        verify("completed copy")?;
        if IMPORT_CALLS.with(|calls| calls.borrow().finishes) != finishes + 2 {
            return Err("successful import did not complete producer reads".into());
        }
        let fence = device.create_completion_fence()?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let completion = loop {
            match device.poll_completion_fence(fence) {
                Ok(true) => break Ok(()),
                Ok(false) if Instant::now() < deadline => std::thread::yield_now(),
                Ok(false) => break Err("completion fence deadline exceeded".into()),
                Err(error) => break Err(Box::<dyn std::error::Error>::from(error)),
            }
        };
        device.delete_completion_fence(fence);
        completion?;
        if device.poll_completion_fence(fence).is_ok() {
            return Err("released completion fence accepted".into());
        }
        verify("completion fence")?;
        eprintln!(
            "device-import: GL sentinels preserved on metadata/destination/EGL/FBO failure and success; real glFinish observed before callback return; completion fence retired"
        );
        Ok(())
    })();
    original.restore(&gl);
    // SAFETY: Restored original bindings before deleting all exclusively owned
    // sentinels; copies/fence have finished and no names escape this function.
    unsafe {
        (gl.delete_textures)(2, textures.as_ptr());
        (gl.delete_samplers)(2, samplers.as_ptr());
        (gl.delete_framebuffers)(2, framebuffers.as_ptr());
        (gl.delete_vaos)(1, &vao);
    }
    result
}
