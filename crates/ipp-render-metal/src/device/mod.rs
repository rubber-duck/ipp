//! Typed Rust bindings to Metal and the renderer's GPU data layouts.
use crate::analytic_surface::AnalyticPrimitive;
use objc2::{rc::Retained, runtime::ProtocolObject};
use objc2_metal::*;
use std::{cell::RefCell, rc::Rc};

mod canvas;
mod frame;
mod pipelines;
mod resources;
mod uploads;

pub(super) use frame::Frame;
use resources::Backend;

type Gpu<T> = Retained<ProtocolObject<T>>;
type Buffer = Gpu<dyn MTLBuffer>;
type Texture = Gpu<dyn MTLTexture>;
type Pipeline = Gpu<dyn MTLRenderPipelineState>;
type Encoder = ProtocolObject<dyn MTLRenderCommandEncoder>;

pub(super) enum Object {
    Buffer(Buffer),
    Texture(Texture),
    Program(Rc<pipelines::Programs>),
    Recording(Vec<canvas::Batch>),
}

impl Object {
    fn buffer(&self) -> &Buffer {
        let Self::Buffer(b) = self else {
            unreachable!("expected buffer")
        };
        b
    }

    fn texture(&self) -> &Texture {
        let Self::Texture(t) = self else {
            unreachable!("expected texture")
        };
        t
    }

    fn program(&self) -> &Rc<pipelines::Programs> {
        let Self::Program(p) = self else {
            unreachable!("expected pipeline")
        };
        p
    }
}

pub(super) struct Device(RefCell<Rc<Backend>>);
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct MeshVertex {
    pub(super) position: [f32; 4],
    pub(super) color_weight: [f32; 4],
    pub(super) uv: [f32; 4],
    pub(super) joints: [u32; 4],
    pub(super) joint_weights: [f32; 4],
    pub(super) normal: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::NoUninit)]
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
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
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
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable, Default)]
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

#[derive(Clone)]
pub(super) struct Target {
    color: Texture,
    depth: Texture,
    owner: Option<Texture>,
    size: [u32; 2],
}

impl Target {
    pub(super) fn size(&self) -> [u32; 2] {
        self.size
    }

    pub(super) fn layered(&self) -> bool {
        self.owner.is_some()
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

#[cfg(test)]
mod tests {
    use super::*;
    use objc2_quartz_core::CAMetalLayer;

    #[test]
    fn presents_to_core_animation_and_preserves_offscreen_pixels() {
        objc2::rc::autoreleasepool(|_| {
            let device = Device::new().unwrap();
            assert!(device.resize(32, 24));
            let layer = CAMetalLayer::new();
            let mut extent = layer.drawableSize();
            extent.width = 32.;
            extent.height = 24.;
            layer.setDrawableSize(extent);
            device.set_layer(Some(layer.clone())).unwrap();
            assert!(layer.device().is_some());
            let mut pixels = vec![0; 32 * 24 * 4];
            let mut check = |clear, expected| {
                let mut frame = device.frame().unwrap();
                assert!(frame.pass(None, clear, None, None));
                frame.finish(Some(&mut pixels), None).unwrap();
                assert!(
                    pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .all(|rgba| *rgba == expected)
                );
            };
            check([0., 1., 0., 1.], [0, 255, 0, 255]);

            // An embedding host may temporarily remove the layer device. Readback still draws the new frame.
            layer.setDevice(None);
            assert!(layer.nextDrawable().is_none());
            check([1., 0., 0., 1.], [255, 0, 0, 255]);
            device.set_layer(None).unwrap();
            check([0., 0., 1., 1.], [0, 0, 255, 255]);
        });
    }
}
