//! WebGPU pipelines and storage buffers for batched Canvas coverage.
use crate::device::{COLOR, DEPTH, texture_descriptor};
use ipp_render_scene::canvas::CanvasVertex;
use std::rc::Rc;

const DEFAULT_PAINT: &str =
    "vec4 ipp_paint(vec2 point, vec2 size, vec4 color, float edge, vec4 data) { return color; }";

pub(super) struct CanvasDevice {
    pub layouts: [wgpu::BindGroupLayout; 3],
    empty_owner: wgpu::TextureView,
    pub nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
    vertex: wgpu::ShaderModule,
    layout: wgpu::PipelineLayout,
    pub clear: wgpu::RenderPipeline,
    pub ordinary: Rc<CanvasProgram>,
    pub masks: Option<crate::path_masks::MaskPipelines>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CanvasPipeline {
    Overlay,
    DepthTested,
    Analytic,
    Owner,
    Panel,
    Screen,
}

pub(super) struct CanvasProgram([wgpu::RenderPipeline; 6]);

impl std::ops::Index<CanvasPipeline> for CanvasProgram {
    type Output = wgpu::RenderPipeline;

    fn index(&self, kind: CanvasPipeline) -> &Self::Output {
        &self.0[kind as usize]
    }
}

pub(super) fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRIBUTES: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
        0=>Float32x4, 1=>Float32x2, 2=>Float32x4, 3=>Float32x3,
        4=>Float32x3, 5=>Float32x3, 6=>Uint32
    ];
    wgpu::VertexBufferLayout {
        array_stride: size_of::<CanvasVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRIBUTES,
    }
}

impl CanvasDevice {
    pub fn new(device: &wgpu::Device, can_mask: bool) -> Self {
        let layouts = std::array::from_fn(|index| {
            let image = index == 1;
            let mut entries = vec![wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: if image {
                    wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float {
                            filterable: true,
                        },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    }
                } else {
                    wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage {
                            read_only: true,
                        },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    }
                },
                count: None,
            }];
            if index < 2 {
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(if image {
                        wgpu::SamplerBindingType::Filtering
                    } else {
                        wgpu::SamplerBindingType::NonFiltering
                    }),
                    count: None,
                });
            }
            if index == 1 {
                entries.push(wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float {
                            filterable: false,
                        },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                });
            }
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Canvas resources"),
                entries: &entries,
            })
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Canvas"),
            bind_group_layouts: &layouts.each_ref().map(Some),
            ..Default::default()
        });
        let vertex = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Canvas projection"),
            source: wgpu::ShaderSource::Wgsl(include_str!("canvas.wgsl").into()),
        });
        let nearest = device.create_sampler(&Default::default());
        let linear = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let ordinary = Rc::new(Self::compile(device, &layout, &vertex, DEFAULT_PAINT));
        let clear_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Repaint damaged pixels"),
            source: wgpu::ShaderSource::Wgsl(include_str!("clear.wgsl").into()),
        });
        let clear = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Clear color and depth in scissor"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &clear_shader,
                entry_point: Some("vertex"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &clear_shader,
                entry_point: Some("fragment"),
                compilation_options: Default::default(),
                targets: &[Some(COLOR.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let empty_owner = device
            .create_texture(&texture_descriptor(
                "Empty layer owner",
                [1, 1],
                wgpu::TextureFormat::Rgba8Unorm,
                wgpu::TextureUsages::TEXTURE_BINDING,
            ))
            .create_view(&Default::default());
        let masks = can_mask
            .then(|| crate::path_masks::MaskPipelines::new(device, &layouts, &layout, &vertex));
        Self {
            masks,
            empty_owner,
            clear,
            layouts,
            layout,
            vertex,
            ordinary,
            nearest,
            linear,
        }
    }

    pub async fn paint(
        &self,
        device: &wgpu::Device,
        function: &str,
    ) -> Result<Rc<CanvasProgram>, String> {
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let program = Self::compile(device, &self.layout, &self.vertex, function);
        if let Some(error) = scope.pop().await {
            return Err(error.to_string());
        }
        Ok(Rc::new(program))
    }

    fn compile(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        vertex: &wgpu::ShaderModule,
        function: &str,
    ) -> CanvasProgram {
        let source = include_str!("canvas.frag")
            .replace("// IPP_PATH_COVERAGE", include_str!("path-coverage.glsl"))
            .replace("// IPP_INTERSECTION", include_str!("intersection.glsl"))
            .replace("// IPP_PAINT_FUNCTION", function);

        use CanvasPipeline::*;
        CanvasProgram(
            [Overlay, DepthTested, Analytic, Owner, Panel, Screen].map(|kind| {
                let fragment = crate::shaders::shader_module(
                    device,
                    wgpu::ShaderModuleDescriptor {
                        label: Some("Canvas coverage"),
                        source: wgpu::ShaderSource::Glsl {
                            shader: std::borrow::Cow::Borrowed(&source),
                            stage: wgpu::naga::ShaderStage::Fragment,
                            defines: match kind {
                                Analytic => &[("IPP_ANALYTIC", "1")],
                                Owner => &[("IPP_OWNER", "1")],
                                Panel => &[("IPP_PANEL", "1")],
                                Screen => &[("IPP_ANALYTIC", "1"), ("IPP_SCREEN", "1")],
                                Overlay | DepthTested => &[],
                            },
                        },
                    },
                );
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Canvas composition"),
                    layout: Some(layout),
                    vertex: wgpu::VertexState {
                        module: vertex,
                        entry_point: Some("canvas_vertex"),
                        compilation_options: Default::default(),
                        buffers: &[Some(vertex_layout())],
                    },
                    primitive: Default::default(),
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(if matches!(kind, DepthTested | Analytic | Screen) {
                            wgpu::CompareFunction::LessEqual
                        } else {
                            wgpu::CompareFunction::Always
                        }),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &fragment,
                        entry_point: Some("main"),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: if kind == Owner {
                                wgpu::TextureFormat::Rgba8Unorm
                            } else {
                                COLOR
                            },
                            blend: if kind == Owner {
                                None
                            } else {
                                Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING)
                            },
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            }),
        )
    }

    pub fn bind(&self, device: &wgpu::Device, view: &wgpu::TextureView) -> wgpu::BindGroup {
        self.bind_with_owner(device, view, &self.empty_owner)
    }

    pub fn bind_with_owner(
        &self,
        device: &wgpu::Device,
        view: &wgpu::TextureView,
        owner: &wgpu::TextureView,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Canvas image"),
            layout: &self.layouts[1],
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.linear),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(owner),
                },
            ],
        })
    }
}

pub(super) struct RecordBuffer {
    buffer: wgpu::Buffer,
    pub bind: wgpu::BindGroup,
}

impl RecordBuffer {
    const STRIDE: u64 = size_of::<[f32; 4]>() as u64;

    pub fn limit(device: &wgpu::Device) -> usize {
        let limits = device.limits();
        (limits
            .max_buffer_size
            .min(limits.max_storage_buffer_binding_size)
            / Self::STRIDE) as usize
    }

    pub fn new(gpu: &crate::device::Graphics, group: usize, records: usize) -> Self {
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Canvas records"),
            size: records as u64 * Self::STRIDE,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }];
        if group == 0 {
            entries.push(wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&gpu.canvas.nearest),
            });
        }
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Canvas records"),
            layout: &gpu.canvas.layouts[group],
            entries: &entries,
        });
        Self {
            buffer,
            bind,
        }
    }

    pub fn capacity(&self) -> usize {
        (self.buffer.size() / Self::STRIDE) as usize
    }

    pub fn write(&self, queue: &wgpu::Queue, start: usize, values: &[[f32; 4]]) {
        queue.write_buffer(
            &self.buffer,
            start as u64 * Self::STRIDE,
            bytemuck::cast_slice(values),
        );
    }
}
