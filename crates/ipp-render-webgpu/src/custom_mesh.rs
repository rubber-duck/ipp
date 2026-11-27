//! Compile the authored mesh interface to native WebGPU pipelines.
use crate::{
    assets::MeshLayout,
    device::Graphics,
    shaders::{BACKEND, Program, WebGpuShader},
};
use ipp_core::{
    HostRuntime, WorldPublicationId,
    services::asset_management::{
        MAX_JOINTS,
        formats::{
            ipp_shader::{IppShaderTarget, lower},
            shader::{ShaderDefinition, ShaderParameterKind as Kind},
        },
    },
    systems::render::{PublishedCustomMaterial, PublishedMaterialProperty},
};
use std::{collections::BTreeMap, fmt::Write};

pub(super) struct MaterialProgram {
    pub shadow: Option<wgpu::RenderPipeline>,
    pub plots: [wgpu::RenderPipeline; 2],
    pub pipelines: [wgpu::RenderPipeline; 2],
    pub layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
}

pub(super) async fn compile(
    gpu: &Graphics,
    definition: &ShaderDefinition,
) -> Result<MaterialProgram, String> {
    let stage = definition
        .backend_source(BACKEND)
        .ok_or("No portable or GLSL material source")?;
    if stage.fragment.trim().is_empty() {
        return Err("Material has no fragment body".into());
    }
    let (vertex, outgoing) = varying_declarations(&stage.vertex, "out")?;
    let (fragment, incoming) = varying_declarations(&stage.fragment, "in")?;
    if incoming != outgoing || incoming.len() > 4 {
        return Err("Material varying declarations do not match or exceed capacity".into());
    }
    let lower_stage = |body: &str| {
        if definition.selects_portable(BACKEND) {
            lower(body, IppShaderTarget::GlslEs300)
        } else {
            Ok(body.to_owned())
        }
    };
    let stages = wgpu::ShaderStages::VERTEX_FRAGMENT;
    let mut bindings = vec![wgpu::BindGroupLayoutEntry {
        binding: 0,
        visibility: stages,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }];
    let mut declarations = String::new();
    let mut words = 1;
    let mut textures = 0;
    for (name, kind) in &definition.parameters {
        if *kind == Kind::Texture2D {
            if textures == 0 {
                declarations.push_str("layout(set=3,binding=1) uniform sampler image_sampler;\n");
                bindings.push(wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: stages,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                });
            }
            let binding = 2 + textures;
            writeln!(declarations,"layout(set=3,binding={binding}) uniform texture2D image_{name};\n#define p_{name} sampler2D(image_{name},image_sampler)").unwrap();
            bindings.push(wgpu::BindGroupLayoutEntry {
                binding,
                visibility: stages,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float {
                        filterable: true,
                    },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
            textures += 1;
        } else {
            let value = match kind {
                Kind::U32 => format!("params[{words}].x"),
                Kind::I32 => format!("int(params[{words}].x)"),
                Kind::Bool => format!("(params[{words}].x!=0u)"),
                Kind::F32 | Kind::Vec2 | Kind::Vec3 | Kind::Vec4 => format!(
                    "uintBitsToFloat(params[{words}]){}",
                    match kind {
                        Kind::F32 => ".x",
                        Kind::Vec2 => ".xy",
                        Kind::Vec3 => ".xyz",
                        _ => "",
                    }
                ),
                Kind::Mat2 | Kind::Mat3 | Kind::Mat4 => {
                    let n = record_count(*kind);
                    let columns = (words..words + n)
                        .map(|i| format!("uintBitsToFloat(params[{i}]).{}", &"xyzw"[..n]))
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("mat{n}({columns})")
                }
                Kind::Texture2D => unreachable!(),
            };
            writeln!(declarations, "#define p_{name} ({value})").unwrap();
            words += record_count(*kind);
        }
    }
    if words > 65 || textures > 8 {
        return Err("Material parameter capacity exceeded".into());
    }
    let mut io = String::new();
    for (i, (ty, name)) in [
        ("vec3", "v_position"),
        ("vec3", "v_normal"),
        ("vec3", "v_color"),
        ("vec2", "v_uv"),
        ("float", "v_weight"),
    ]
    .into_iter()
    .chain(outgoing.iter().map(|(name, ty)| (*ty, *name)))
    .enumerate()
    {
        writeln!(io, "layout(location={i}) VARYING {ty} {name};").unwrap();
    }
    let lights = ipp_render_scene::lighting::MAX_DOMAIN_LIGHTS * 4;
    let lit = u8::from(definition.recipe.features & 8 != 0);
    let header = format!(
        "#version 450\n#define IPP_MAX_JOINTS {MAX_JOINTS}\n#define IPP_MAX_LIGHTS {}\n#define IPP_SHADER_INTERFACE_VERSION 1\n#define IPP_PASS_SHADOW 0\n#define IPP_RECEIVES_LIGHT {lit}\nlayout(set=0,binding=0,std140) uniform Camera {{mat4 projection;vec4 u_camera;vec4 ambient_count;vec4 u_lights[{lights}];}};\n#define u_light_count int(ambient_count.w)\n#define u_ambient ambient_count.xyz\nlayout(set=3,binding=0,std140) uniform Properties {{uvec4 params[{words}];}};\n{declarations}",
        lights / 4
    );
    let vertex_body = if vertex.trim().is_empty() {
        String::from("void materialVertex(){ippDefaultVertex();}")
    } else {
        lower_stage(&vertex)?
    };
    let vertex_inverse = crate::shaders::inverse_functions(&vertex_body);
    let fragment_inverse = crate::shaders::inverse_functions(&fragment);
    let vert = format!(
        "{header}{vertex_inverse}{}\n{}\n{vertex_body}\nvoid main(){{materialVertex();gl_Position.z=(gl_Position.z+gl_Position.w)*0.5;}}",
        io.replace("VARYING", "out"),
        include_str!("custom.vert")
    );
    let frag = format!(
        "{header}{fragment_inverse}{}\nlayout(location=0) out vec4 out_color;\n{}\n{}\n#ifndef IPP_ALPHA_MODE\n#define IPP_ALPHA_MODE params[0].x\n#endif\nvoid main(){{vec4 c=materialFragment();uint mode=IPP_ALPHA_MODE;if(mode==1u && c.a<uintBitsToFloat(params[0].y))discard;out_color=vec4(clamp(c.rgb,0.0,1.0),mode==2u?clamp(c.a,0.0,1.0):1.0);}}",
        io.replace("VARYING", "in"),
        include_str!("custom-lighting.glsl"),
        lower_stage(&fragment)?
    );
    let device = &gpu.device;
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let shader = |source: String, stage| {
        crate::shaders::shader_module(
            device,
            wgpu::ShaderModuleDescriptor {
                label: Some("Authored mesh shader"),
                source: wgpu::ShaderSource::Glsl {
                    shader: source.into(),
                    stage,
                    defines: &[],
                },
            },
        )
    };
    let shadow_vert = (definition.recipe.features & 16 != 0)
        .then(|| vert.replace("#define IPP_PASS_SHADOW 0", "#define IPP_PASS_SHADOW 1"));
    let shadow_frag = shadow_vert.as_ref().map(|_| {
        frag.replace("#define IPP_PASS_SHADOW 0", "#define IPP_PASS_SHADOW 1")
            .replace(
                "layout(location=0) out vec4 out_color;",
                "layout(location=0) out float shadow_depth; vec4 out_color;",
            )
            .replace(
                "out_color=vec4(clamp(c.rgb",
                "shadow_depth=gl_FragCoord.z;out_color=vec4(clamp(c.rgb",
            )
    });
    let plot = shader(
        vert.replacen("#version 450", "#version 450\n#define IPP_PLOT 1", 1),
        wgpu::naga::ShaderStage::Vertex,
    );
    let vertex = shader(vert, wgpu::naga::ShaderStage::Vertex);
    // Blended materials never use alpha cutoff. Excluding the discard in this
    // pipeline lets the GPU test depth before evaluating expensive optics.
    let blended = shader(
        frag.replacen("#version 450", "#version 450\n#define IPP_ALPHA_MODE 2u", 1),
        wgpu::naga::ShaderStage::Fragment,
    );
    let fragment = shader(frag, wgpu::naga::ShaderStage::Fragment);
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Material parameters"),
        entries: &bindings,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Custom mesh"),
        bind_group_layouts: &[
            Some(&gpu.mesh_layouts[0]),
            Some(&gpu.mesh_layouts[1]),
            Some(&gpu.mesh_layouts[2]),
            Some(&layout),
        ],
        ..Default::default()
    });
    let pipelines = |vertex, mesh_layout| {
        std::array::from_fn(|blend| {
            Graphics::mesh_pipeline(
                device,
                &pipeline_layout,
                vertex,
                if blend == 1 {
                    &blended
                } else {
                    &fragment
                },
                ["main", "main"],
                blend,
                mesh_layout,
            )
        })
    };
    let plots = pipelines(&plot, MeshLayout::PaintedPlot);
    let pipelines = pipelines(&vertex, MeshLayout::Full);
    let shadow = shadow_vert.zip(shadow_frag).map(|(v, f)| {
        let v = shader(v, wgpu::naga::ShaderStage::Vertex);
        let f = shader(f, wgpu::naga::ShaderStage::Fragment);
        Graphics::mesh_pipeline(
            device,
            &pipeline_layout,
            &v,
            &f,
            ["main", "main"],
            3,
            MeshLayout::Full,
        )
    });
    if let Some(error) = scope.pop().await {
        return Err(error.to_string());
    }
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    Ok(MaterialProgram {
        pipelines,
        plots,
        layout,
        sampler,
        shadow,
    })
}

fn varying_declarations<'a>(
    source: &'a str,
    qualifier: &str,
) -> Result<(String, BTreeMap<&'a str, &'a str>), String> {
    let mut result = String::new();
    let mut varyings = BTreeMap::new();
    for line in source.lines() {
        let mut words = line.split_ascii_whitespace();
        if words.next() == Some(qualifier) {
            let ty = words.next().ok_or("Missing material varying type")?;
            let name = words
                .next()
                .and_then(|name| name.strip_suffix(';'))
                .ok_or("Invalid material varying declaration")?;
            let mut characters = name.chars();
            if !characters
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
                || !characters.all(|value| value.is_ascii_alphanumeric() || value == '_')
                || varyings.insert(name, ty).is_some()
            {
                return Err("Invalid or repeated material varying name".into());
            }
        } else {
            writeln!(result, "{line}").unwrap();
        }
    }
    Ok((result, varyings))
}

fn record_count(kind: Kind) -> usize {
    match kind {
        Kind::Mat2 => 2,
        Kind::Mat3 => 3,
        Kind::Mat4 => 4,
        _ => 1,
    }
}

pub(super) struct CustomDraw<'a> {
    pub program: &'a MaterialProgram,
    pub values: Vec<[u32; 4]>,
    pub textures: Vec<&'a wgpu::TextureView>,
    pub blended: bool,
}

impl<'a> CustomDraw<'a> {
    pub fn resolve(
        host: &'a HostRuntime,
        publication: WorldPublicationId,
        material: &PublishedCustomMaterial,
        attributes: u32,
    ) -> Option<Self> {
        let shader =
            crate::scene::resource::<WebGpuShader>(host, publication, material.shader?.to_u64())?;
        let Program::Mesh(program) = shader.program.as_ref()? else {
            return None;
        };
        let definition = &shader.definition;
        if definition.required_attributes & attributes != definition.required_attributes {
            return None;
        }
        let mut values = vec![[material.alpha_mode, material.alpha_cutoff.to_bits(), 0, 0]];
        let mut textures = Vec::new();
        for (name, kind) in &definition.parameters {
            match material.properties.get(name.as_str())? {
                PublishedMaterialProperty::Resource(Some(key)) if *kind == Kind::Texture2D => {
                    textures.push(
                        &crate::scene::resource::<crate::assets::WebGpuTexture>(
                            host,
                            publication,
                            key.to_u64(),
                        )?
                        .texture
                        .as_ref()?
                        .2,
                    );
                }
                PublishedMaterialProperty::Value(value)
                    if Kind::from_value(value).ok()? == *kind =>
                {
                    if let Some(floats) = value.floats() {
                        let count = record_count(*kind);
                        for column in floats.chunks(floats.len() / count) {
                            let mut record = [0; 4];
                            for (output, input) in record.iter_mut().zip(column) {
                                *output = input.to_bits();
                            }
                            values.push(record);
                        }
                    } else {
                        let word = match value {
                            ipp_core::DynamicValue::I32(value) => *value as u32,
                            ipp_core::DynamicValue::U32(value) => *value,
                            ipp_core::DynamicValue::Bool(value) => u32::from(*value),
                            _ => return None,
                        };
                        values.push([word, 0, 0, 0]);
                    }
                }
                _ => return None,
            }
        }
        Some(Self {
            program,
            values,
            textures,
            blended: material.alpha_mode == 2,
        })
    }

    pub fn matches(&self, other: &Self) -> bool {
        std::ptr::eq(self.program, other.program)
            && self.values == other.values
            && self.textures == other.textures
    }
}
