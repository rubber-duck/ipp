use super::{frame::Frame, pipelines::*, *};
use crate::analytic_surface::AnalyticMapping;
use objc2::rc::autoreleasepool;
use objc2_io_surface::IOSurfaceRef;
use objc2_quartz_core::CAMetalLayer;
use std::{collections::VecDeque, ffi::c_void, ptr::NonNull};

const MESH: &str = include_str!("../mesh.metal");
const CANVAS: &str = include_str!("../canvas.metal");
const WEB: &str = include_str!("../web_image.metal");
const PRESENT: &str = include_str!("../present.metal");

pub(super) struct Submission {
    pub(super) commands: Gpu<dyn MTLCommandBuffer>,
    pub(super) uploads: Vec<Buffer>,
}

pub(super) struct ColumnMap {
    pub(super) mappings: Vec<AnalyticMapping>,
    pub(super) extent: [u32; 2],
    pub(super) buffer: Buffer,
    pub(super) producer: Option<Gpu<dyn MTLCommandBuffer>>,
}

pub(super) struct Backend {
    pub(super) gpu: Gpu<dyn MTLDevice>,
    pub(super) queue: Gpu<dyn MTLCommandQueue>,
    pub(super) programs: Programs,
    pub(super) clear: Pipeline,
    pub(super) depth: [Gpu<dyn MTLDepthStencilState>; 3],
    pub(super) columns: Gpu<dyn MTLComputePipelineState>,
    web: Gpu<dyn MTLComputePipelineState>,
    pub(super) white: Texture,
    pub(super) empty_owner: Texture,
    pub(super) empty_depth: Texture,
    pub(super) empty_columns: Buffer,
    pub(super) root: RefCell<Option<Target>>,
    pub(super) submitted: RefCell<VecDeque<Submission>>,
    pub(super) pool: RefCell<Vec<Buffer>>,
    pub(super) maps: RefCell<VecDeque<ColumnMap>>,
    pub(super) previous: RefCell<(Vec<AnalyticMapping>, [u32; 2])>,
    pub(super) layer: RefCell<Option<(Retained<CAMetalLayer>, Pipeline)>>,
}

fn descriptor(
    format: MTLPixelFormat,
    w: usize,
    h: usize,
    storage: MTLStorageMode,
    usage: MTLTextureUsage,
) -> Retained<MTLTextureDescriptor> {
    // SAFETY: Callers provide positive, bounded 2D extents and a concrete color/depth format.
    let d = unsafe {
        MTLTextureDescriptor::texture2DDescriptorWithPixelFormat_width_height_mipmapped(
            format, w, h, false,
        )
    };
    d.setStorageMode(storage);
    d.setUsage(usage);
    d
}

fn texture(
    gpu: &ProtocolObject<dyn MTLDevice>,
    format: MTLPixelFormat,
    size: [u32; 2],
    storage: MTLStorageMode,
    usage: MTLTextureUsage,
) -> Option<Texture> {
    gpu.newTextureWithDescriptor(&descriptor(
        format,
        size[0] as usize,
        size[1] as usize,
        storage,
        usage,
    ))
}

fn rgba(gpu: &ProtocolObject<dyn MTLDevice>, w: u32, h: u32, pixels: &[u8]) -> Option<Texture> {
    if w == 0 || h == 0 || w > 16384 || h > 16384 || pixels.len() != w as usize * h as usize * 4 {
        return None;
    }
    let t = texture(
        gpu,
        ROOT,
        [w, h],
        MTLStorageMode::Shared,
        MTLTextureUsage::ShaderRead,
    )?;
    // SAFETY: The validated tightly packed bytes cover this entire base mip. The new texture is unpublished.
    unsafe {
        t.replaceRegion_mipmapLevel_withBytes_bytesPerRow(
            region(w as usize, h as usize),
            0,
            NonNull::from_ref(&pixels[0]).cast(),
            w as usize * 4,
        )
    };
    Some(t)
}

pub(super) fn region(w: usize, h: usize) -> MTLRegion {
    MTLRegion {
        origin: MTLOrigin {
            x: 0,
            y: 0,
            z: 0,
        },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    }
}

pub(super) fn buffer(gpu: &ProtocolObject<dyn MTLDevice>, data: &[u8]) -> Option<Buffer> {
    // SAFETY: Metal synchronously copies the exact live slice into a separately owned allocation.
    unsafe {
        gpu.newBufferWithBytes_length_options(
            NonNull::from_ref(data.first()?).cast(),
            data.len(),
            MTLResourceOptions::StorageModeShared,
        )
    }
}

impl Backend {
    fn target(&self, size: [u32; 2], format: MTLPixelFormat, layered: bool) -> Option<Target> {
        let usage = MTLTextureUsage::RenderTarget | MTLTextureUsage::ShaderRead;
        let color = texture(&self.gpu, format, size, MTLStorageMode::Private, usage)?;
        let storage = if self.gpu.supportsFamily(MTLGPUFamily::Apple1) {
            MTLStorageMode::Memoryless
        } else {
            MTLStorageMode::Private
        };
        let depth = texture(
            &self.gpu,
            MTLPixelFormat::Depth32Float,
            size,
            storage,
            MTLTextureUsage::RenderTarget,
        )?;
        let owner = if layered {
            Some(texture(
                &self.gpu,
                MTLPixelFormat::R32Uint,
                size,
                MTLStorageMode::Private,
                usage,
            )?)
        } else {
            None
        };
        Some(Target {
            color,
            depth,
            owner,
            size,
        })
    }

    pub(super) fn retire(&self, need_slot: bool) -> Result<(), String> {
        let mut queue = self.submitted.borrow_mut();
        while let Some(old) = queue.front() {
            if need_slot && queue.len() >= 3 {
                old.commands.waitUntilCompleted();
            }
            match old.commands.status() {
                MTLCommandBufferStatus::Completed => {}
                MTLCommandBufferStatus::Error => {
                    let failure = command_error(&old.commands);
                    queue.pop_front();
                    return Err(failure);
                }
                _ => break,
            }
            let retired = queue.pop_front().unwrap();
            self.recycle(retired.uploads);
        }
        Ok(())
    }

    pub(super) fn recycle(&self, buffers: Vec<Buffer>) {
        let mut pool = self.pool.borrow_mut();
        for b in buffers {
            if pool.len() < 12 && b.length() <= 4 * 1024 * 1024 {
                pool.push(b)
            }
        }
    }
}

pub(super) fn command_error(cmd: &ProtocolObject<dyn MTLCommandBuffer>) -> String {
    cmd.error()
        .map_or_else(|| "Metal command failed".into(), |e| e.to_string())
}

impl Device {
    pub(crate) fn new() -> Result<Self, String> {
        autoreleasepool(|_| {
            let gpu = MTLCreateSystemDefaultDevice().ok_or("No Metal device")?;
            let queue = gpu.newCommandQueue().ok_or("No Metal command queue")?;
            let lib = compile(&gpu, &format!("{MESH}\n{CANVAS}\n{WEB}"))?;
            let mut programs = Programs::new(
                &gpu,
                &lib,
                &(OPAQUE..=PLOT_ALPHA).collect::<Vec<_>>(),
                false,
            )?;
            programs.shadow = Some(simple_pipeline(
                &gpu,
                &lib,
                "mesh_vertex",
                None,
                None,
                true,
            )?);
            let clear = simple_pipeline(
                &gpu,
                &lib,
                "canvas_clear_vertex",
                Some("canvas_clear_fragment"),
                Some(LINEAR),
                true,
            )?;
            let compute = |name| {
                gpu.newComputePipelineStateWithFunction_error(&*function(&lib, name)?)
                    .map_err(|e| e.to_string())
            };
            let columns = compute("canvas_columns")?;
            let web = compute("import_web_image")?;
            let depth_state = |compare, write| {
                let d = MTLDepthStencilDescriptor::new();
                d.setDepthCompareFunction(compare);
                d.setDepthWriteEnabled(write);
                gpu.newDepthStencilStateWithDescriptor(&d)
                    .ok_or("Metal depth state unavailable")
            };
            let depth = [
                depth_state(MTLCompareFunction::Less, true)?,
                depth_state(MTLCompareFunction::LessEqual, false)?,
                depth_state(MTLCompareFunction::Always, false)?,
            ];
            let white = rgba(&gpu, 1, 1, &[255; 4]).ok_or("Metal fallback color")?;
            let empty = |fmt| {
                texture(
                    &gpu,
                    fmt,
                    [1; 2],
                    MTLStorageMode::Private,
                    MTLTextureUsage::ShaderRead,
                )
                .ok_or("Metal fallback texture")
            };
            let empty_owner = empty(MTLPixelFormat::R32Uint)?;
            let empty_depth = empty(MTLPixelFormat::Depth32Float)?;
            let empty_columns = buffer(&gpu, &[0; 64]).ok_or("Metal fallback columns")?;
            Ok(Self(RefCell::new(Rc::new(Backend {
                gpu,
                queue,
                programs,
                clear,
                depth,
                columns,
                web,
                white,
                empty_owner,
                empty_depth,
                empty_columns,
                root: Default::default(),
                submitted: Default::default(),
                pool: Default::default(),
                maps: Default::default(),
                previous: Default::default(),
                layer: Default::default(),
            }))))
        })
    }

    pub(crate) fn canvas_program(&self, paint: &str) -> Result<Object, String> {
        let d = self.0.borrow();
        let lib = compile(
            &d.gpu,
            &format!(
                "{MESH}\n{}",
                CANVAS.replace("/* IPP_AUTHORED_PAINT */", paint)
            ),
        )?;
        Ok(Object::Program(Rc::new(Programs::new(
            &d.gpu,
            &lib,
            &[ANALYTIC, AFFINE, LAYERS, COLUMN_SHAPE, CYLINDER_SHAPE],
            false,
        )?)))
    }

    pub(crate) fn material_program(&self, material: &str, shadow: bool) -> Result<Object, String> {
        let d = self.0.borrow();
        let lib = |pass| {
            compile(
                &d.gpu,
                &format!("#define IPP_PASS_SHADOW {pass}\n{MESH}\n{CANVAS}\n{material}"),
            )
        };
        let mut p = Programs::new(&d.gpu, &*lib(0)?, &[0, 1], true)?;
        if shadow {
            p.shadow = Some(simple_pipeline(
                &d.gpu,
                &*lib(1)?,
                "custom_vertex",
                Some("custom_fragment"),
                None,
                true,
            )?);
        }
        Ok(Object::Program(Rc::new(p)))
    }

    pub(crate) fn presentation_layer(&self) -> Option<Retained<CAMetalLayer>> {
        self.0
            .borrow()
            .layer
            .borrow()
            .as_ref()
            .map(|(layer, _)| layer.clone())
    }

    pub(crate) fn replace(&self, replacement: Self) {
        *self.0.borrow_mut() = replacement.0.into_inner();
    }

    pub(crate) fn name(&self) -> String {
        self.0.borrow().gpu.name().to_string()
    }

    #[cfg(feature = "instrumentation")]
    pub(crate) fn gpu_timing_available() -> bool {
        objc2::available!(macos = 10.15)
    }

    pub(crate) fn resize(&self, w: u32, h: u32) -> bool {
        let d = self.0.borrow();
        if d.root.borrow().as_ref().is_some_and(|t| t.size == [w, h]) {
            return true;
        }
        let Some(t) = d.target([w, h], ROOT, false) else {
            return false;
        };
        *d.root.borrow_mut() = Some(t);
        true
    }

    pub(crate) fn set_layer(&self, layer: Option<Retained<CAMetalLayer>>) -> Result<(), String> {
        let d = self.0.borrow();
        let value = if let Some(layer) = layer {
            let lib = compile(&d.gpu, PRESENT)?;
            let format = MTLPixelFormat::BGRA8Unorm_sRGB;
            let pipeline = simple_pipeline(
                &d.gpu,
                &lib,
                "present_vertex",
                Some("present_fragment"),
                Some(format),
                false,
            )?;
            layer.setDevice(Some(&d.gpu));
            layer.setPixelFormat(format);
            Some((layer, pipeline))
        } else {
            None
        };
        *d.layer.borrow_mut() = value;
        Ok(())
    }

    pub(crate) fn mesh<V: bytemuck::Pod>(
        &self,
        vertices: &[V],
        indices: &[u16],
    ) -> Option<MeshBuffers> {
        let d = self.0.borrow();
        Some(MeshBuffers {
            vertices: Object::Buffer(buffer(&d.gpu, bytemuck::cast_slice(vertices))?),
            indices: Some(Object::Buffer(buffer(
                &d.gpu,
                bytemuck::cast_slice(indices),
            )?)),
            count: indices.len(),
        })
    }

    pub(crate) fn plot_triangles<const N: usize>(&self, v: &[[f32; N]]) -> Option<MeshBuffers> {
        Some(MeshBuffers {
            vertices: Object::Buffer(buffer(&self.0.borrow().gpu, bytemuck::cast_slice(v))?),
            indices: None,
            count: v.len(),
        })
    }

    pub(crate) fn texture(&self, w: u32, h: u32, pixels: &[u8]) -> Option<Object> {
        rgba(&self.0.borrow().gpu, w, h, pixels).map(Object::Texture)
    }

    pub(crate) fn path(&self, r: &[[f32; 4]]) -> Option<Object> {
        buffer(&self.0.borrow().gpu, bytemuck::cast_slice(r)).map(Object::Buffer)
    }

    pub(crate) fn path_page(&self, n: usize) -> Option<Object> {
        self.0
            .borrow()
            .gpu
            .newBufferWithLength_options(n.checked_mul(16)?, MTLResourceOptions::StorageModeShared)
            .map(Object::Buffer)
    }

    pub(crate) fn write_path(&self, page: &Object, start: usize, records: &[[f32; 4]]) -> bool {
        let b = page.buffer();
        let Some(offset) = start.checked_mul(16) else {
            return false;
        };
        let bytes = bytemuck::cast_slice::<_, u8>(records);
        if offset > b.length() || bytes.len() > b.length() - offset {
            return false;
        }
        // SAFETY: The path allocator reserves fresh, disjoint ranges and the bounds check covers the entire write.
        unsafe {
            b.contents()
                .cast::<u8>()
                .as_ptr()
                .add(offset)
                .copy_from_nonoverlapping(bytes.as_ptr(), bytes.len())
        };
        true
    }

    pub(crate) fn shadow_texture(&self, size: u32) -> Option<Object> {
        texture(
            &self.0.borrow().gpu,
            MTLPixelFormat::Depth32Float,
            [size; 2],
            MTLStorageMode::Private,
            MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget,
        )
        .map(Object::Texture)
    }

    pub(crate) fn target(&self, size: [u32; 2], layered: bool) -> Option<Target> {
        self.0.borrow().target(size, LINEAR, layered)
    }

    pub(crate) fn frame(&self) -> Result<Frame, String> {
        Frame::new(self.0.borrow().clone())
    }

    pub(crate) unsafe fn import_web_image(
        &self,
        surface: *mut c_void,
        [w, h]: [u32; 2],
        budget: usize,
    ) -> Result<Object, String> {
        // SAFETY: The caller lends an IOSurfaceRef until conversion completes before return.
        let surface =
            unsafe { surface.cast::<IOSurfaceRef>().as_ref() }.ok_or("missing IOSurface")?;
        if surface.plane_count() > 1
            || surface.width() != w as usize
            || surface.height() != h as usize
            || surface.pixel_format() != u32::from_be_bytes(*b"BGRA")
            || surface.alloc_size() > budget
        {
            return Err("Invalid accelerated web image".into());
        }
        let d = self.0.borrow();
        let desc = descriptor(
            MTLPixelFormat::BGRA8Unorm,
            w as usize,
            h as usize,
            MTLStorageMode::Shared,
            MTLTextureUsage::ShaderRead,
        );
        let source = d
            .gpu
            .newTextureWithDescriptor_iosurface_plane(&desc, surface, 0)
            .ok_or("Could not import IOSurface")?;
        let dest = texture(
            &d.gpu,
            LINEAR,
            [w, h],
            MTLStorageMode::Private,
            MTLTextureUsage::ShaderRead | MTLTextureUsage::ShaderWrite,
        )
        .ok_or("Could not allocate converted texture")?;
        let cmd = d.queue.commandBuffer().ok_or("No import command buffer")?;
        #[cfg(feature = "instrumentation")]
        cmd.setLabel(Some(objc2_foundation::ns_string!(
            "IPP IOSurface conversion"
        )));
        let compute = cmd
            .computeCommandEncoder()
            .ok_or("No import compute encoder")?;
        compute.setComputePipelineState(&d.web);
        // SAFETY: Both textures have the formats and extents required by the import kernel and live through completion.
        unsafe {
            compute.setTexture_atIndex(Some(&source), 0);
            compute.setTexture_atIndex(Some(&dest), 1);
        }
        let lane = d.web.threadExecutionWidth();
        compute.dispatchThreads_threadsPerThreadgroup(
            region(w as usize, h as usize).size,
            MTLSize {
                width: lane,
                height: 8.min(d.web.maxTotalThreadsPerThreadgroup() / lane),
                depth: 1,
            },
        );
        compute.endEncoding();
        cmd.commit();
        cmd.waitUntilCompleted();
        if cmd.status() != MTLCommandBufferStatus::Completed {
            return Err(command_error(&cmd));
        }
        Ok(Object::Texture(dest))
    }
}
