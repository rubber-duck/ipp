//! Metal adapter selection, WebGPU pipelines and offscreen attachments.

use std::sync::{Arc, Mutex};

use crate::assets::MeshLayout;
use wgpu::util::DeviceExt;

use ipp_core::services::asset_management::MAX_JOINTS;
use ipp_render_scene::lighting::MAX_DOMAIN_LIGHTS;

pub(super) const PALETTE_BYTES: u64 = (MAX_JOINTS * size_of::<[f32; 16]>()) as u64;
pub(super) const COLOR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub(super) const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth24Plus;

pub(super) struct Graphics {
    #[cfg(target_arch = "wasm32")]
    pub web_images: std::cell::RefCell<crate::web_images::WebImages>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub pipeline: wgpu::RenderPipeline,
    pub plot: [wgpu::RenderPipeline; 2],
    pub rigid_unlit: [wgpu::RenderPipeline; 2],
    pub shadow_layout: wgpu::BindGroupLayout,
    pub empty_shadow: ShadowAtlas,
    pub shadow_pipeline: wgpu::RenderPipeline,
    pub transparent: [wgpu::RenderPipeline; 2],
    pub quad: crate::assets::MeshBuffers,
    pub name: String,
    pub mesh_layouts: [wgpu::BindGroupLayout; 3],
    pub canvas: crate::canvas_device::CanvasDevice,
    sampler: wgpu::Sampler,
    error: Arc<Mutex<Option<String>>>,
}

impl Graphics {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new() -> Result<Self, String> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        futures_lite::future::block_on(Self::open(&instance, None))
    }

    pub async fn open(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
    ) -> Result<Self, String> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: surface,
                ..Default::default()
            })
            .await
            .map_err(|error| format!("WebGPU adapter: {error}"))?;

        let info = adapter.get_info();
        #[cfg(not(target_arch = "wasm32"))]
        if info.backend != wgpu::Backend::Metal
            || !matches!(
                info.device_type,
                wgpu::DeviceType::IntegratedGpu | wgpu::DeviceType::DiscreteGpu
            )
        {
            return Err(format!("Expected a hardware Metal adapter: {info:?}"));
        }

        let can_mask = crate::path_masks::MaskPipelines::supported(&adapter);
        let name = format!("{:?} {}", info.backend, info.name);
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("IPP WebGPU"),
                required_limits: wgpu::Limits {
                    max_storage_buffers_per_shader_stage: if can_mask {
                        3
                    } else {
                        2
                    },
                    max_storage_buffer_binding_size: adapter
                        .limits()
                        .max_storage_buffer_binding_size
                        .min(256 << 20),
                    ..wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits())
                },
                ..Default::default()
            })
            .await
            .map_err(|e| e.to_string())?;

        let error = Arc::new(Mutex::new(None));
        let loss = Arc::clone(&error);
        device.set_device_lost_callback(move |reason, message| {
            *loss.lock().unwrap() = Some(format!("Device lost ({reason:?}): {message}"));
        });
        let uncaptured = Arc::clone(&error);
        device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
            uncaptured
                .lock()
                .unwrap()
                .get_or_insert_with(|| e.to_string());
        }));

        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let source = (include_str!("mesh.wgsl").to_owned() + include_str!("shadows.wgsl"))
            .replace("IPP_MAX_JOINTS", &MAX_JOINTS.to_string())
            .replace("IPP_MAX_DOMAIN_LIGHTS", &MAX_DOMAIN_LIGHTS.to_string());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("IPP mesh"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let stages = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let uniform_layout = |label| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: stages,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            })
        };
        let images = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Mesh texture"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: stages,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float {
                            filterable: true,
                        },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: stages,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let mesh_layouts = [
            uniform_layout("Camera uniform"),
            images,
            uniform_layout("Joint palette"),
        ];
        let shadow_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Spotlight shadow atlas"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float {
                        filterable: false,
                    },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Camera"),
            bind_group_layouts: &[
                Some(&mesh_layouts[0]),
                Some(&mesh_layouts[1]),
                Some(&mesh_layouts[2]),
                Some(&shadow_layout),
            ],
            ..Default::default()
        });
        let pipeline = Self::mesh_pipeline(
            &device,
            &layout,
            &shader,
            &shader,
            ["mesh_vertex", "mesh_fragment"],
            0,
            MeshLayout::Full,
        );
        let rigid_unlit = ["color_fragment", "textured_fragment"].map(|entry| {
            Self::mesh_pipeline(
                &device,
                &layout,
                &shader,
                &shader,
                ["rigid_vertex", entry],
                0,
                MeshLayout::Full,
            )
        });
        let transparent = std::array::from_fn(|i| {
            Self::mesh_pipeline(
                &device,
                &layout,
                &shader,
                &shader,
                ["mesh_vertex", "mesh_fragment"],
                i + 1,
                MeshLayout::Full,
            )
        });
        let shadow_pipeline = Self::mesh_pipeline(
            &device,
            &layout,
            &shader,
            &shader,
            ["mesh_vertex", "shadow_fragment"],
            3,
            MeshLayout::Full,
        );
        let plot = std::array::from_fn(|blend| {
            Self::mesh_pipeline(
                &device,
                &layout,
                &shader,
                &shader,
                ["plot_vertex", "color_fragment"],
                blend,
                MeshLayout::Plot,
            )
        });
        let empty_shadow = ShadowAtlas::new(&device, &shadow_layout, 1);
        let vertices = ipp_render_scene::particles::quad();
        let quad = crate::assets::MeshBuffers {
            layout: MeshLayout::Full,
            vertices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Particle quad"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            indices: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Particle indices"),
                contents: bytemuck::cast_slice(&[0u16, 1, 2, 0, 2, 3]),
                usage: wgpu::BufferUsages::INDEX,
            }),
            count: 6,
        };
        let canvas = crate::canvas_device::CanvasDevice::new(&device, can_mask);
        if let Some(error) = scope.pop().await {
            return Err(error.to_string());
        }

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            ..Default::default()
        });
        Ok(Self {
            #[cfg(target_arch = "wasm32")]
            web_images: Default::default(),
            device,
            queue,
            name,
            pipeline,
            canvas,
            mesh_layouts,
            rigid_unlit,
            plot,
            transparent,
            quad,
            shadow_pipeline,
            shadow_layout,
            empty_shadow,
            sampler,
            error,
        })
    }

    pub fn mesh_pipeline(
        device: &wgpu::Device,
        layout: &wgpu::PipelineLayout,
        vertex: &wgpu::ShaderModule,
        fragment: &wgpu::ShaderModule,
        entries: [&str; 2],
        blend: usize,
        mesh_layout: MeshLayout,
    ) -> wgpu::RenderPipeline {
        let vertices = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4, 3 => Uint32x4, 4 => Float32x4, 5 => Float32x4];
        let poses = [
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 6,
                format: wgpu::VertexFormat::Float32x4,
            },
            wgpu::VertexAttribute {
                offset: std::mem::offset_of!(ipp_render_scene::mesh::MeshVertex, normal) as u64,
                shader_location: 7,
                format: wgpu::VertexFormat::Float32x4,
            },
        ];
        let plot_vertices = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
        let painted_vertices = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x2, 5 => Float32x3];
        let (stride, attributes): (usize, &[_]) = match mesh_layout {
            MeshLayout::Full => (size_of::<ipp_render_scene::mesh::MeshVertex>(), &vertices),
            MeshLayout::Plot => (size_of::<[f32; 6]>(), &plot_vertices),
            MeshLayout::PaintedPlot => (size_of::<[f32; 11]>(), &painted_vertices),
        };
        let instances = wgpu::vertex_attr_array![8 => Float32x4, 9 => Float32x4, 10 => Float32x4, 11 => Float32x4, 12 => Float32x4, 13 => Float32x4, 14 => Float32x4];
        let shadow_targets = [Some(wgpu::TextureFormat::R32Float.into())];
        let color_targets = [Some(wgpu::ColorTargetState {
            format: COLOR,
            blend: match blend {
                0 => None,
                1 => Some(wgpu::BlendState::ALPHA_BLENDING),
                _ => Some(wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::OVER,
                }),
            },
            write_mask: wgpu::ColorWrites::ALL,
        })];
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("IPP Camera"),
            layout: Some(layout),
            vertex: wgpu::VertexState {
                module: vertex,
                entry_point: Some(entries[0]),
                compilation_options: Default::default(),
                buffers: &[
                    Some(wgpu::VertexBufferLayout {
                        array_stride: stride as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes,
                    }),
                    (mesh_layout == MeshLayout::Full).then_some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<ipp_render_scene::mesh::MeshVertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &poses,
                    }),
                    Some(wgpu::VertexBufferLayout {
                        array_stride: size_of::<crate::scene::Instance>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &instances,
                    }),
                ],
            },
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH,
                depth_write_enabled: Some(blend == 0 || blend == 3),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: if entries[1].is_empty() {
                None
            } else {
                Some(wgpu::FragmentState {
                    module: fragment,
                    entry_point: Some(entries[1]),
                    compilation_options: Default::default(),
                    targets: if blend == 3 {
                        &shadow_targets
                    } else {
                        &color_targets
                    },
                })
            },
            multiview_mask: None,
            cache: None,
        })
    }

    pub fn check(&self) -> Result<(), String> {
        self.device
            .poll(wgpu::PollType::Poll)
            .map_err(|e| e.to_string())?;
        self.error.lock().unwrap().clone().map_or(Ok(()), Err)
    }

    /// Attribute a rejected asset upload to that asset while retaining its CPU metadata.
    pub async fn upload<T>(&self, create: impl FnOnce() -> Option<T>) -> Result<T, String> {
        let memory = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let validation = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let value = create();
        let invalid = validation.pop();
        let exhausted = memory.pop();
        let (invalid, exhausted) = futures_lite::future::zip(invalid, exhausted).await;
        if let Some(error) = invalid.or(exhausted) {
            return Err(error.to_string());
        }
        value.ok_or_else(|| "Unsupported WebGPU asset dimensions".into())
    }

    pub fn uniform(&self, group: u32, size: u64) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("IPP draw uniform"),
            size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("IPP draw uniform"),
            layout: &self.mesh_layouts[group as usize],
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        (buffer, bind)
    }

    pub fn texture(
        &self,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Option<(wgpu::BindGroup, wgpu::BindGroup, wgpu::TextureView)> {
        let limit = self.device.limits().max_texture_dimension_2d;
        if !(1..=limit).contains(&width) || !(1..=limit).contains(&height) {
            return None;
        }
        let texture = self.device.create_texture_with_data(
            &self.queue,
            &texture_descriptor(
                "IPP albedo",
                [width, height],
                COLOR,
                wgpu::TextureUsages::TEXTURE_BINDING,
            ),
            wgpu::util::TextureDataOrder::LayerMajor,
            rgba,
        );
        let view = texture.create_view(&Default::default());
        let canvas = self.canvas.bind(&self.device, &view);
        Some((
            self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("IPP albedo"),
                layout: &self.mesh_layouts[1],
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            }),
            canvas,
            view,
        ))
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait(&self, index: wgpu::SubmissionIndex) -> Result<(), String> {
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: Some(std::time::Duration::from_secs(10)),
            })
            .map_err(|e| e.to_string())?;
        self.error.lock().unwrap().clone().map_or(Ok(()), Err)
    }
}

pub(super) fn texture_descriptor(
    label: &str,
    [width, height]: [u32; 2],
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
) -> wgpu::TextureDescriptor<'_> {
    wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    }
}

#[derive(Clone)]
pub(super) struct Target {
    pub owner: Option<wgpu::TextureView>,
    pub image: wgpu::BindGroup,
    pub color: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub depth: wgpu::TextureView,
}

impl Target {
    pub fn new(gpu: &Graphics, size: [u32; 2]) -> Self {
        Self::canvas_image(gpu, size, false)
    }

    pub fn canvas_image(gpu: &Graphics, size: [u32; 2], layered: bool) -> Self {
        let color = gpu.device.create_texture(&texture_descriptor(
            "IPP root",
            size,
            COLOR,
            wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
        ));
        let depth = gpu.device.create_texture(&texture_descriptor(
            "IPP depth",
            size,
            DEPTH,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TRANSIENT_ATTACHMENT,
        ));
        let view = color.create_view(&Default::default());
        let owner = layered.then(|| {
            gpu.device
                .create_texture(&texture_descriptor(
                    "Canvas layer owners",
                    size,
                    wgpu::TextureFormat::Rgba8Unorm,
                    wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                ))
                .create_view(&Default::default())
        });
        let image = match &owner {
            Some(owner) => gpu.canvas.bind_with_owner(&gpu.device, &view, owner),
            None => gpu.canvas.bind(&gpu.device, &view),
        };
        Self {
            owner,
            image,
            view,
            color,
            depth: depth.create_view(&Default::default()),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn copy(
        &self,
        gpu: &Graphics,
        encoder: &mut wgpu::CommandEncoder,
    ) -> (wgpu::Buffer, usize) {
        let extent = self.color.size();
        let stride = (extent.width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("IPP capture"),
            size: u64::from(stride) * u64::from(extent.height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            self.color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: None,
                },
            },
            extent,
        );
        (buffer, stride as usize)
    }
}

#[derive(Clone)]
pub(super) struct ShadowAtlas {
    pub color: wgpu::TextureView,
    pub view: wgpu::TextureView,
    pub bind: wgpu::BindGroup,
    pub size: u32,
}
impl ShadowAtlas {
    pub fn new(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, size: u32) -> Self {
        let texture = device.create_texture(&texture_descriptor(
            "Spotlight depth",
            [size, size],
            DEPTH,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
        ));
        let view = texture.create_view(&Default::default());
        let color = device
            .create_texture(&texture_descriptor(
                "Spotlight sampleable depths",
                [size, size],
                wgpu::TextureFormat::R32Float,
                wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            ))
            .create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Spotlight shadow map"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&color),
            }],
        });
        Self {
            view,
            color,
            bind,
            size,
        }
    }
}
