//! Owned Metal objects and checked Rust boundary to the Objective-C API.

use std::{
    cell::RefCell,
    ffi::{CStr, CString, c_char, c_void},
    mem::{size_of, size_of_val},
    ptr::{self, NonNull},
};

use crate::analytic_surface::{AnalyticDraw, AnalyticMapping, AnalyticPrimitive};
use crate::lighting::{Lighting, MAX_DOMAIN_LIGHTS};

unsafe extern "C" {
    fn ipp_metal_device(
        source: *const c_char,
        domain_light_limit: usize,
        message: *mut c_char,
        capacity: usize,
    ) -> *mut c_void;
    fn ipp_metal_canvas_program(
        device: *mut c_void,
        source: *const c_char,
        domain_light_limit: usize,
        message: *mut c_char,
        capacity: usize,
    ) -> *mut c_void;
    fn ipp_metal_material_program(
        device: *mut c_void,
        source: *const c_char,
        shadow_source: *const c_char,
        domain_light_limit: usize,
        message: *mut c_char,
        capacity: usize,
    ) -> *mut c_void;
    fn ipp_metal_web_image(
        device: *mut c_void,
        surface: *mut c_void,
        width: u32,
        height: u32,
        budget: usize,
        message: *mut c_char,
        capacity: usize,
    ) -> *mut c_void;
    fn ipp_metal_release(object: *mut c_void);
    fn ipp_metal_name(device: *mut c_void, name: *mut c_char, capacity: usize);
    fn ipp_metal_resize(device: *mut c_void, width: u32, height: u32) -> bool;
    fn ipp_metal_buffer(device: *mut c_void, bytes: *const c_void, length: usize) -> *mut c_void;
    fn ipp_metal_write_buffer(
        buffer: *mut c_void,
        offset: usize,
        bytes: *const c_void,
        length: usize,
    ) -> bool;
    fn ipp_metal_texture(
        device: *mut c_void,
        width: u32,
        height: u32,
        pixels: *const u8,
    ) -> *mut c_void;
    fn ipp_metal_target(device: *mut c_void, width: u32, height: u32, layered: bool)
    -> *mut c_void;
    fn ipp_metal_frame(device: *mut c_void) -> *mut c_void;
    fn ipp_metal_shadow_texture(device: *mut c_void, size: u32) -> *mut c_void;
    fn ipp_metal_shadow(frame: *mut c_void, texture: *mut c_void) -> bool;
    fn ipp_metal_shadow_tile(frame: *mut c_void, x: u32, y: u32, size: u32);
    fn ipp_metal_pass(
        frame: *mut c_void,
        target: *mut c_void,
        background: *const f32,
        lighting: *const Lighting,
        lighting_bytes: usize,
        shadow: *mut c_void,
        damage: *const u32,
    ) -> bool;
    fn ipp_metal_canvas_columns(
        frame: *mut c_void,
        mappings: *const AnalyticMapping,
        mapping_bytes: usize,
        count: usize,
        width: u32,
        height: u32,
    ) -> bool;
    fn ipp_metal_lighting(frame: *mut c_void, lighting: *const Lighting, length: usize);
    fn ipp_metal_presentation_extent(frame: *mut c_void, extent: *mut u32) -> bool;
    fn ipp_metal_begin_canvas_record(frame: *mut c_void) -> bool;
    fn ipp_metal_end_canvas_record(frame: *mut c_void, bytes: *mut usize) -> *mut c_void;
    fn ipp_metal_replay_canvas(frame: *mut c_void, record: *mut c_void) -> bool;
    fn ipp_metal_canvas(
        frame: *mut c_void,
        vertices: *const CanvasVertex,
        vertex_bytes: usize,
        count: usize,
        paint: *const CanvasPaint,
        paint_bytes: usize,
        bitmap: *mut c_void,
        image: *mut c_void,
        path: *mut c_void,
        path_start: u32,
        program: *mut c_void,
        parameters: *const [f32; 4],
        parameter_bytes: usize,
        mapping: *const AnalyticMapping,
        mapping_bytes: usize,
        uses_columns: bool,
        is_cylinder: bool,
        analytic: *const AnalyticDraw,
        analytic_bytes: usize,
        edges: *const [f32; 4],
        edge_bytes: usize,
        depth_test: bool,
        layer_owner: u32,
        layer_select: u32,
    ) -> bool;
    fn ipp_metal_draw(
        frame: *mut c_void,
        vertices: *mut c_void,
        indices: *mut c_void,
        index_count: usize,
        pose: *mut c_void,
        texture: *mut c_void,
        materials: *const Material,
        material_bytes: usize,
        instances: usize,
        joints: *const [f32; 16],
        joint_bytes: usize,
        blend: Blend,
        program: *mut c_void,
        parameters: *const [u32; 4],
        parameter_bytes: usize,
        textures: *const *mut c_void,
        texture_count: usize,
    ) -> bool;
    fn ipp_metal_finish(
        frame: *mut c_void,
        pixels: *mut u8,
        length: usize,
        message: *mut c_char,
        capacity: usize,
        gpu_seconds: *mut f64,
        merged_canvas_draws: *mut u32,
    ) -> bool;

    #[cfg(feature = "instrumentation")]
    fn ipp_metal_gpu_timing_available() -> bool;
}

pub(super) struct Object(NonNull<c_void>);

impl Object {
    fn new(object: *mut c_void) -> Option<Self> {
        NonNull::new(object).map(Self)
    }

    fn ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }
}

impl Drop for Object {
    fn drop(&mut self) {
        // SAFETY: Object owns one retained reference from the device shim. GPU
        // commands retain encoded buffers and textures independently.
        unsafe { ipp_metal_release(self.ptr()) };
    }
}

#[repr(C)]
pub(super) struct MeshVertex {
    pub(super) position: [f32; 4],
    pub(super) color_weight: [f32; 4],
    pub(super) uv: [f32; 4],
    pub(super) joints: [u32; 4],
    pub(super) joint_weights: [f32; 4],
    pub(super) normal: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct Material {
    pub(super) clip_from_mesh: [f32; 16],
    pub(super) world_from_mesh: [f32; 16],
    pub(super) color: [f32; 4],
    pub(super) options: [f32; 4],
    pub(super) surface: [f32; 4],
    pub(super) effects: [f32; 4],
}

impl Material {
    /// Bound one upload while allowing an entire compatible emitter to share a draw.
    pub(super) const MAX_INSTANCES: usize = 1024;
}

#[repr(C)]
pub(super) struct CanvasVertex {
    pub(super) clip: [f32; 4],
    pub(super) local: [f32; 2],
    pub(super) world: [f32; 4],
    pub(super) right: [f32; 3],
    pub(super) down: [f32; 3],
    pub(super) normal: [f32; 3],
    pub(super) slot: u32,
}

#[repr(C)]
#[derive(Default)]
pub(super) struct CanvasPaint {
    pub(super) tint: [f32; 4],
    pub(super) bounds: [f32; 4],
    pub(super) fill: [[f32; 4]; 2],
    pub(super) gradient: [f32; 4],
    pub(super) border: [f32; 4],
    pub(super) options: [f32; 4],
    pub(super) shape: [f32; 4],
    pub(super) segment: [[f32; 4]; 2],
    pub(super) cut: [f32; 4],
    pub(super) accent: [f32; 4],
    pub(super) glow_color: [f32; 4],
    pub(super) glow: [f32; 4],
    pub(super) checker: [[f32; 4]; 2],
    pub(super) section: [f32; 4],
    pub(super) clip: [f32; 4],
    pub(super) paint_origin_scale: [f32; 4],
    pub(super) paint_size_opacity: [f32; 4],
    pub(super) paint_data: [f32; 4],
}

pub(super) struct Target(Object, [u32; 2], bool);

impl Target {
    pub(super) fn size(&self) -> [u32; 2] {
        self.1
    }

    pub(super) fn layered(&self) -> bool {
        self.2
    }
}

pub(super) enum CanvasBinding<'a> {
    Bitmap(&'a Object),
    Target(&'a Target),
    Layer {
        target: &'a Target,
        id: u32,
    },
    Path {
        buffer: &'a Object,
        start: u32,
    },
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Blend {
    Opaque,
    Alpha,
    Additive,
}

pub(super) struct MeshBuffers {
    vertices: Object,
    indices: Option<Object>,
    count: usize,
}

pub(super) struct PaintParameters<'a> {
    pub(super) program: &'a Object,
    pub(super) values: &'a [[f32; 4]],
}

pub(super) struct CanvasCoverage<'a> {
    pub(super) analytic: Option<&'a AnalyticPrimitive<'a>>,
    pub(super) depth_test: bool,
    pub(super) layer_owner: u32,
}

pub(super) struct MeshStreams<'a> {
    pub(super) mesh: &'a MeshBuffers,
    pub(super) pose: Option<&'a MeshBuffers>,
    pub(super) joints: &'a [[f32; 16]],
}

pub(super) struct MeshParameters<'a> {
    pub(super) program: &'a Object,
    pub(super) values: &'a [[u32; 4]],
    pub(super) textures: &'a [&'a Object],
}

const MESH_SOURCE: &str = include_str!("mesh.metal");
const CANVAS_SOURCE: &str = include_str!("canvas.metal");

pub(super) struct Device(RefCell<Object>);

impl Device {
    #[cfg(feature = "instrumentation")]
    pub(super) fn gpu_timing_available() -> bool {
        // SAFETY: The shim performs only an operating-system availability check.
        unsafe { ipp_metal_gpu_timing_available() }
    }

    pub(super) fn new() -> Result<Self, String> {
        let source = CStr::from_bytes_with_nul(
            concat!(
                include_str!("mesh.metal"),
                "\n",
                include_str!("canvas.metal"),
                "\n",
                include_str!("web_image.metal"),
                "\0"
            )
            .as_bytes(),
        )
        .expect("embedded MSL must contain no NUL");
        let mut message = [0; 4096];
        // SAFETY: Source is a live C string and the mutable error array is writable
        // for the stated size. A returned object carries one retained reference.
        let object = unsafe {
            ipp_metal_device(
                source.as_ptr(),
                MAX_DOMAIN_LIGHTS,
                message.as_mut_ptr(),
                message.len(),
            )
        };
        Object::new(object)
            .map(|object| Self(RefCell::new(object)))
            .ok_or_else(|| read_message(&message))
    }

    pub(super) fn canvas_program(&self, paint: &str) -> Result<Object, String> {
        let canvas = CANVAS_SOURCE.replace("/* IPP_AUTHORED_PAINT */", paint);
        let source =
            CString::new(format!("{MESH_SOURCE}\n{canvas}")).map_err(|error| error.to_string())?;
        let mut message = [0; 4096];
        // SAFETY: The device and nul-terminated source are live during compilation.
        // The writable error region and owned result follow the device shim ABI.
        let object = unsafe {
            ipp_metal_canvas_program(
                self.0.borrow().ptr(),
                source.as_ptr(),
                MAX_DOMAIN_LIGHTS,
                message.as_mut_ptr(),
                message.len(),
            )
        };
        Object::new(object).ok_or_else(|| read_message(&message))
    }

    pub(super) fn material_program(&self, material: &str, shadow: bool) -> Result<Object, String> {
        let source = |pass| {
            CString::new(format!(
                "#define IPP_PASS_SHADOW {pass}\n{MESH_SOURCE}\n{CANVAS_SOURCE}\n{material}"
            ))
            .map_err(|error| error.to_string())
        };
        let surface_source = source(0)?;
        let shadow_source = shadow.then(|| source(1)).transpose()?;
        let mut message = [0; 4096];
        // SAFETY: The source, error span and device stay live until compilation
        // completes. The successful pointer carries one independently retained reference.
        let object = unsafe {
            ipp_metal_material_program(
                self.0.borrow().ptr(),
                surface_source.as_ptr(),
                shadow_source
                    .as_ref()
                    .map_or(ptr::null(), |source| source.as_ptr()),
                MAX_DOMAIN_LIGHTS,
                message.as_mut_ptr(),
                message.len(),
            )
        };
        Object::new(object).ok_or_else(|| read_message(&message))
    }

    pub(super) fn replace(&self, replacement: Self) {
        *self.0.borrow_mut() = replacement.0.into_inner();
    }

    pub(super) fn name(&self) -> String {
        let mut message = [0; 256];
        // SAFETY: The device remains live while the shim writes a bounded name.
        unsafe { ipp_metal_name(self.0.borrow().ptr(), message.as_mut_ptr(), message.len()) };
        read_message(&message)
    }

    pub(super) fn resize(&self, width: u32, height: u32) -> bool {
        // SAFETY: The owned device is used only on its Host thread and no other
        // encoder is active. Metal validates the texture allocations.
        unsafe { ipp_metal_resize(self.0.borrow().ptr(), width, height) }
    }

    pub(super) fn mesh(&self, vertices: &[MeshVertex], indices: &[u16]) -> Option<MeshBuffers> {
        // SAFETY: Both initialized slices live until Metal has copied the exact
        // byte ranges. Each successfully allocated buffer is independently owned.
        unsafe {
            Some(MeshBuffers {
                vertices: Object::new(ipp_metal_buffer(
                    self.0.borrow().ptr(),
                    vertices.as_ptr().cast(),
                    size_of_val(vertices),
                ))?,
                indices: Some(Object::new(ipp_metal_buffer(
                    self.0.borrow().ptr(),
                    indices.as_ptr().cast(),
                    size_of_val(indices),
                ))?),
                count: indices.len(),
            })
        }
    }

    /// Generated Plot triangle streams already follow draw order.
    pub(super) fn plot_triangles<const N: usize>(
        &self,
        vertices: &[[f32; N]],
    ) -> Option<MeshBuffers> {
        if vertices.is_empty() {
            return None;
        }

        // SAFETY: Fixed-size float arrays have no padding. Metal copies the
        // initialized slice before returning the independently owned buffer.
        let vertices_buffer = unsafe {
            Object::new(ipp_metal_buffer(
                self.0.borrow().ptr(),
                vertices.as_ptr().cast(),
                size_of_val(vertices),
            ))?
        };
        Some(MeshBuffers {
            vertices: vertices_buffer,
            indices: None,
            count: vertices.len(),
        })
    }

    pub(super) fn texture(&self, width: u32, height: u32, rgba: &[u8]) -> Option<Object> {
        if width > 16_384 || height > 16_384 {
            return None;
        }
        let expected = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        if expected != rgba.len() {
            return None;
        }

        // SAFETY: The checked byte range covers all tightly packed rows. Metal
        // copies the pixels during the call and returns an owned texture.
        Object::new(unsafe {
            ipp_metal_texture(self.0.borrow().ptr(), width, height, rgba.as_ptr())
        })
    }

    pub(super) unsafe fn import_web_image(
        &self,
        surface: *mut c_void,
        [width, height]: [u32; 2],
        budget: usize,
    ) -> Result<Object, String> {
        let mut message = [0; 4096];
        // SAFETY: The caller lends a validated IOSurface through the synchronous
        // conversion and supplies its allocation budget. The device remains live
        // and the result owns independent private Metal texture storage.
        let object = unsafe {
            ipp_metal_web_image(
                self.0.borrow().ptr(),
                surface,
                width,
                height,
                budget,
                message.as_mut_ptr(),
                message.len(),
            )
        };
        Object::new(object).ok_or_else(|| read_message(&message))
    }

    pub(super) fn path(&self, records: &[[f32; 4]]) -> Option<Object> {
        // SAFETY: The slice is initialized and remains live for the bounded
        // synchronous copy into a newly allocated Metal buffer.
        Object::new(unsafe {
            ipp_metal_buffer(
                self.0.borrow().ptr(),
                records.as_ptr().cast(),
                size_of_val(records),
            )
        })
    }

    pub(super) fn path_page(&self, records: usize) -> Option<Object> {
        // SAFETY: A null source requests an owned shared allocation without a
        // CPU read. Only initialized ranges are exposed to shaders.
        Object::new(unsafe {
            ipp_metal_buffer(
                self.0.borrow().ptr(),
                ptr::null(),
                records.checked_mul(size_of::<[f32; 4]>())?,
            )
        })
    }

    pub(super) fn write_path(&self, page: &Object, start: usize, records: &[[f32; 4]]) -> bool {
        // SAFETY: The caller exclusively reserves this range for one append.
        // The bridge bounds the write against the live shared buffer. No
        // submitted shader can reference the unallocated range, and the
        // initialized source remains live for the synchronous copy.
        unsafe {
            ipp_metal_write_buffer(
                page.ptr(),
                start * size_of::<[f32; 4]>(),
                records.as_ptr().cast(),
                size_of_val(records),
            )
        }
    }

    pub(super) fn shadow_texture(&self, size: u32) -> Option<Object> {
        // SAFETY: The retained object uses the live device. The renderer bounds
        // the atlas size before encoding a pass with it.
        Object::new(unsafe { ipp_metal_shadow_texture(self.0.borrow().ptr(), size) })
    }

    pub(super) fn target(&self, [width, height]: [u32; 2], layered: bool) -> Option<Target> {
        // SAFETY: The device is live and the shim allocates independently owned attachments.
        Object::new(unsafe { ipp_metal_target(self.0.borrow().ptr(), width, height, layered) })
            .map(|target| Target(target, [width, height], layered))
    }

    pub(super) fn frame(&self) -> Option<Frame> {
        // SAFETY: The device is live and the frame retains its device and root attachment.
        Object::new(unsafe { ipp_metal_frame(self.0.borrow().ptr()) }).map(Frame)
    }
}

pub(super) struct Frame(Object);

impl Frame {
    pub(super) fn shadow(&mut self, texture: &Object) -> bool {
        // SAFETY: The texture is a live Metal depth allocation from the same device.
        unsafe { ipp_metal_shadow(self.0.ptr(), texture.ptr()) }
    }

    pub(super) fn shadow_tile(&mut self, x: u32, y: u32, size: u32) {
        // SAFETY: Coordinates are derived from the active atlas size and fit its texture.
        unsafe { ipp_metal_shadow_tile(self.0.ptr(), x, y, size) };
    }

    pub(super) fn pass(
        &mut self,
        target: Option<&Target>,
        background: [f32; 4],
        lighting: Option<&Lighting>,
        shadow: Option<&Object>,
    ) -> bool {
        let unlit = Lighting::default();
        let lighting = lighting.unwrap_or(&unlit);
        // SAFETY: All handles are live and owned by the same device. Metal retains
        // attachments and copies clear values and lighting data during encoding.
        unsafe {
            ipp_metal_pass(
                self.0.ptr(),
                target.map_or(ptr::null_mut(), |target| target.0.ptr()),
                background.as_ptr(),
                lighting,
                size_of::<Lighting>(),
                shadow.map_or(ptr::null_mut(), Object::ptr),
                ptr::null(),
            )
        }
    }

    pub(super) fn repaint_canvas(
        &mut self,
        target: &Target,
        lighting: &Lighting,
        rect: [u32; 4],
    ) -> bool {
        debug_assert!(!target.layered());
        debug_assert!(rect[2] > 0 && rect[3] > 0);
        debug_assert!(
            rect[0] + rect[2] <= target.size()[0] && rect[1] + rect[3] <= target.size()[1]
        );
        // SAFETY: The target owns initialized color storage. The live scissor
        // has four bounded coordinates and the shim copies it while encoding.
        unsafe {
            ipp_metal_pass(
                self.0.ptr(),
                target.0.ptr(),
                [0.0_f32; 4].as_ptr(),
                lighting,
                size_of::<Lighting>(),
                ptr::null_mut(),
                rect.as_ptr(),
            )
        }
    }

    pub(super) fn canvas_columns(
        &mut self,
        mappings: &[AnalyticMapping],
        [width, height]: [u32; 2],
    ) -> bool {
        // SAFETY: The frame retains its device, and Metal copies the entire
        // slice into command-owned storage before this call returns.
        unsafe {
            ipp_metal_canvas_columns(
                self.0.ptr(),
                mappings.as_ptr(),
                size_of_val(mappings),
                mappings.len(),
                width,
                height,
            )
        }
    }

    pub(super) fn lighting(&mut self, lighting: &Lighting) {
        // SAFETY: The render pass is active and Metal copies this initialized
        // constant buffer synchronously into its command encoder.
        unsafe { ipp_metal_lighting(self.0.ptr(), lighting, size_of::<Lighting>()) };
    }

    pub(super) fn presentation_extent(&self) -> Option<[u32; 2]> {
        let mut extent = [0; 2];
        // SAFETY: The live Frame owns the target. The shim writes two u32s.
        unsafe { ipp_metal_presentation_extent(self.0.ptr(), extent.as_mut_ptr()) }
            .then_some(extent)
    }

    pub(super) fn begin_canvas_record(&mut self) -> bool {
        // SAFETY: The active encoder and its mutable recording belong to this Frame.
        unsafe { ipp_metal_begin_canvas_record(self.0.ptr()) }
    }

    pub(super) fn end_canvas_record(&mut self) -> Option<(Object, usize)> {
        let mut bytes = 0;
        // SAFETY: The shim writes one size and transfers ownership of the finished record.
        Object::new(unsafe { ipp_metal_end_canvas_record(self.0.ptr(), &mut bytes) })
            .map(|record| (record, bytes))
    }

    pub(super) fn replay_canvas(&mut self, record: &Object) -> bool {
        // SAFETY: The record is immutable, belongs to this device, and the
        // command buffer retains all referenced resources when it encodes a draw.
        unsafe { ipp_metal_replay_canvas(self.0.ptr(), record.ptr()) }
    }

    pub(super) fn canvas(
        &mut self,
        vertices: &[CanvasVertex],
        paint: &CanvasPaint,
        binding: Option<CanvasBinding<'_>>,
        custom: Option<PaintParameters<'_>>,
        coverage: CanvasCoverage<'_>,
    ) -> bool {
        let CanvasCoverage {
            analytic,
            depth_test,
            layer_owner,
        } = coverage;
        let mut bitmap = ptr::null_mut();
        let mut target = ptr::null_mut();
        let mut path = ptr::null_mut();
        let mut path_start = 0;
        let mut layer_select = 0;
        match binding {
            Some(CanvasBinding::Bitmap(resource)) => bitmap = resource.ptr(),
            Some(CanvasBinding::Target(resource)) => target = resource.0.ptr(),
            Some(CanvasBinding::Layer {
                target: resource,
                id,
            }) => {
                debug_assert!(resource.layered());
                target = resource.0.ptr();
                layer_select = id.saturating_add(1);
            }
            Some(CanvasBinding::Path {
                buffer,
                start,
            }) => {
                path = buffer.ptr();
                path_start = start;
            }
            None => {}
        }
        let (program, values) = custom.map_or((ptr::null_mut(), &[][..]), |custom| {
            (custom.program.ptr(), custom.values)
        });
        assert!(size_of_val(values) <= 4096);
        let edges = analytic.map_or(&[][..], |primitive| primitive.edges.as_slice());
        // SAFETY: The pass is active, buffers and textures belong to this device,
        // and Metal copies the borrowed vertex/paint bytes before this call returns.
        unsafe {
            ipp_metal_canvas(
                self.0.ptr(),
                vertices.as_ptr(),
                size_of_val(vertices),
                vertices.len(),
                paint,
                size_of::<CanvasPaint>(),
                bitmap,
                target,
                path,
                path_start,
                program,
                values.as_ptr(),
                size_of_val(values),
                analytic.map_or(ptr::null(), |primitive| primitive.mapping),
                size_of::<AnalyticMapping>(),
                analytic.is_some_and(|primitive| primitive.mapping.uses_columns()),
                analytic.is_some_and(|primitive| primitive.mapping.is_cylinder()),
                analytic.map_or(ptr::null(), |primitive| &primitive.draw),
                size_of::<AnalyticDraw>(),
                edges.as_ptr(),
                size_of_val(edges),
                depth_test,
                layer_owner,
                layer_select,
            )
        }
    }

    pub(super) fn draw(
        &mut self,
        streams: MeshStreams<'_>,
        texture: Option<&Object>,
        materials: &[Material],
        blend: Blend,
        custom: Option<MeshParameters<'_>>,
    ) -> bool {
        let MeshStreams {
            mesh,
            pose,
            joints,
        } = streams;
        assert!((1..=Material::MAX_INSTANCES).contains(&materials.len()));
        let (program, values, textures) = custom
            .map_or((ptr::null_mut(), &[][..], &[][..]), |draw| {
                (draw.program.ptr(), draw.values, draw.textures)
            });
        assert!(textures.len() <= 8 && size_of_val(values) <= 4096);
        let mut bindings = [ptr::null_mut(); 8];
        for (binding, texture) in bindings.iter_mut().zip(textures) {
            *binding = texture.ptr();
        }
        // SAFETY: The encoder and all Metal buffers are live for encoding.
        // The renderer validates mesh/pose and joint compatibility before this
        // call. Metal retains buffer/texture references and copies inline data.
        unsafe {
            ipp_metal_draw(
                self.0.ptr(),
                mesh.vertices.ptr(),
                mesh.indices.as_ref().map_or(ptr::null_mut(), Object::ptr),
                mesh.count,
                pose.map_or(ptr::null_mut(), |pose| pose.vertices.ptr()),
                texture.map_or(ptr::null_mut(), Object::ptr),
                materials.as_ptr(),
                size_of_val(materials),
                materials.len(),
                joints.as_ptr(),
                size_of_val(joints),
                blend,
                program,
                values.as_ptr(),
                size_of_val(values),
                bindings.as_ptr(),
                textures.len(),
            )
        }
    }

    pub(super) fn finish(
        self,
        capture: Option<&mut [u8]>,
        gpu_seconds: Option<&mut f64>,
    ) -> Result<u32, String> {
        let mut message = [0; 4096];
        let mut merged_canvas_draws = 0;
        let (pixels, length) = capture.map_or((ptr::null_mut(), 0), |pixels| {
            (pixels.as_mut_ptr(), pixels.len())
        });
        // SAFETY: Finishing consumes the frame. The shim validates pixel extent
        // before writing and waits for GPU work before returning borrowed data.
        // Ordinary submissions retain only Metal-owned copies and attachments.
        let completed = unsafe {
            ipp_metal_finish(
                self.0.ptr(),
                pixels,
                length,
                message.as_mut_ptr(),
                message.len(),
                gpu_seconds.map_or(ptr::null_mut(), std::ptr::from_mut),
                &mut merged_canvas_draws,
            )
        };
        completed
            .then_some(merged_canvas_draws)
            .ok_or_else(|| read_message(&message))
    }
}

fn read_message(message: &[c_char]) -> String {
    // SAFETY: Callers pass zero-filled buffers written only by bounded snprintf,
    // so a trailing NUL remains in the supplied allocation.
    unsafe { CStr::from_ptr(message.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}
