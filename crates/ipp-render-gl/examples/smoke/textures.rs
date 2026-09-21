//! Texture scenario using ordinary immutable payloads and completed-frame pixels.

use std::collections::BTreeMap;
use std::path::Path;

use ipp_core::{
    Command, ComponentValue, EntityRef, FieldValue, FieldWrite, MeshAsset, MeshUpload,
    TextureAsset,
    components::{MeshInstance, UnlitMaterial, UnlitTexture},
};
use ipp_render_gl::{RenderDevice, RenderService};

use super::world::{HEIGHT, KEY, MATERIAL, WIDTH, apply, coverage, deliver, float, matches, save};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn validate_uploads<D: RenderDevice>(device: &mut D) {
    for (width, height, pixels) in [
        (0, 1, vec![]),
        (1, 0, vec![]),
        (3, 2, vec![0; 23]),
        (3, 2, vec![0; 25]),
        (3, 2, vec![0; 18]),
        (u32::MAX, u32::MAX, vec![]),
    ] {
        assert!(
            device.create_texture(width, height, &pixels).is_err(),
            "device must reject {width}x{height} with {} bytes",
            pixels.len()
        );
    }
}

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mesh: Vec<u8>,
    texture: Vec<u8>,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    mut rebuild: impl FnMut() -> Result<D>,
    output: &Path,
) -> Result<()> {
    let mut world_host = ipp_core::HostRuntime::new();
    renderer.install(&mut world_host)?;
    let mut world = super::world::fixture_world(&mut world_host)?;
    let texture_type = ComponentValue::UNLIT_TEXTURE;
    apply(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::TRANSFORM,
                fields: vec![],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::UNLIT_MATERIAL,
                fields: vec![
                    float(std::mem::offset_of!(UnlitMaterial, r), MATERIAL[0]),
                    float(std::mem::offset_of!(UnlitMaterial, g), MATERIAL[1]),
                    float(std::mem::offset_of!(UnlitMaterial, b), MATERIAL[2]),
                ],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::MESH_INSTANCE,
                fields: vec![FieldWrite {
                    offset: std::mem::offset_of!(MeshInstance, source) as u32,
                    value: FieldValue::String("fixture:///fixture.mesh".into()),
                }],
            },
            texture_component(EntityRef::Alias(1)),
        ],
    )?;
    assert_eq!(
        super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?.draw_calls,
        0
    );
    save(output, "checker-pending", &capture()?)?;
    let stats = deliver!(renderer, world_host, world, &mesh, Some(&texture))?;
    let item = world.render_items()[0];
    assert!(
        world.mesh(item.mesh).is_none(),
        "GPU upload releases CPU vertex streams"
    );
    let decoded_mesh = MeshAsset::decode(&mesh)?.0;
    let mesh_bytes =
        (decoded_mesh.vertex_bytes() + std::mem::size_of_val(decoded_mesh.indices())) as u32;
    assert_eq!(
        mesh_bytes, 1128,
        "24 UV/normal cube vertices and 36 indices"
    );
    let decoded_texture = TextureAsset::decode(&texture)?.0;
    let asset = &decoded_texture;
    let checker_source = |x, y| checker_rgb(x, y, asset.width(), asset.height());
    assert_payload(asset, checker_source);
    let pixel_bytes = asset.width() * asset.height() * 4;
    let upload_bytes = mesh_bytes + pixel_bytes;
    let entity = EntityRef::Handle(world.render_items()[0].entity);

    assert_eq!(
        (stats.draw_calls, stats.triangles, stats.uploaded_bytes),
        (1, 12, upload_bytes)
    );
    let checker = capture()?;
    save(output, "checker", &checker)?;
    assert_samples(&checker, &decoded_mesh, &decoded_texture, checker_source, 4);
    assert_eq!(
        super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?.uploaded_bytes,
        0
    );

    let world_id = world.id();
    drop(world);
    streaming_reload(
        renderer,
        &mut world_host,
        world_id,
        &texture,
        &checker,
        &mut capture,
        output,
    )?;

    // A replacement context restores the same objects from immutable sources.
    renderer.replace_device(&mut world_host, rebuild()?)?;
    let mut world = world_host.world_mut(world_id).unwrap();
    assert_eq!(
        deliver!(renderer, world_host, world, &mesh, Some(&texture))?.uploaded_bytes,
        upload_bytes
    );
    let recovered = capture()?;
    save(output, "checker-rebuilt", &recovered)?;
    assert_eq!(
        checker, recovered,
        "immutable-source reload must reproduce the frame"
    );

    apply(
        &mut world,
        vec![Command::RemoveComponent {
            entity,
            component: texture_type,
        }],
    )?;
    assert_eq!(
        super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?.uploaded_bytes,
        0
    );
    let solid = capture()?;
    drop(world);
    world_host.flush_resource_lifecycle();
    world = world_host.world_mut(world_id).unwrap();
    save(output, "checker-disabled", &solid)?;
    assert_eq!(coverage(&solid).0, coverage(&checker).0);
    let changed = solid
        .as_chunks::<4>()
        .0
        .iter()
        .zip(checker.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        changed > 3000,
        "texture removal must visibly change the cube"
    );

    apply(&mut world, vec![texture_component(entity)])?;
    let stats = deliver!(renderer, world_host, world, &mesh, Some(&texture))?;
    assert_eq!(
        stats.uploaded_bytes, pixel_bytes,
        "unused texture was evicted; mesh remains resident"
    );
    assert_eq!(capture()?, checker);

    apply(
        &mut world,
        vec![Command::Delete {
            entity,
        }],
    )?;
    assert_eq!(
        super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?.draw_calls,
        0
    );
    let removed = capture()?;
    save(output, "checker-removed", &removed)?;
    assert_eq!(coverage(&removed).0, 0);
    std::fs::write(
        output.join("texture-stats.txt"),
        format!(
            "mesh_bytes={mesh_bytes}\npixel_bytes={}\ninitial_upload_bytes={upload_bytes}\ncache_hit_bytes=0\nrebuild_upload_bytes={upload_bytes}\nretoggle_upload_bytes={}\n",
            pixel_bytes, pixel_bytes,
        ),
    )?;
    drop(world);
    drop(world_host);
    optional_streams(renderer, mesh, &mut capture, &mut rebuild, output)?;
    Ok(())
}

// Exercise the actual GPU loader with bytes withheld and with EOF withheld.
fn streaming_reload<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mut host: &mut ipp_core::HostRuntime,
    world_id: ipp_core::WorldId,
    texture: &[u8],
    expected: &[u8],
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    use ipp_core::services::asset_management::{AssetLoadStatus, AssetSource, STREAM_CAPACITY};

    let mut world = host.world_mut(world_id).unwrap();
    let source = AssetSource {
        kind: ipp_core::TEXTURE_TYPE,
        uri: "fixture:///fixture.texture".into(),
        variant: 0,
    };
    let key = world.asset_resources().find(&source).unwrap();
    let row_bytes = TextureAsset::decode(texture)?.0.width() as usize * 4;
    let first = 16 + row_bytes;
    assert!(
        texture.len() > STREAM_CAPACITY,
        "fixture must exceed source staging"
    );

    world.asset_resources_mut().unload(key);
    drop(world);
    host.flush_resource_lifecycle();
    world = host.world_mut(world_id).unwrap();
    let resource = world.asset_resources().get(key).unwrap();
    assert_eq!(resource.status(), &AssetLoadStatus::Unloaded);
    assert!(resource.data().is_none());
    assert_eq!(resource.stats().resident_bytes, 0);
    assert_eq!(
        super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?.draw_calls,
        0
    );
    let request = world.take_resource_requests().pop().unwrap();
    assert!(request.recovery);
    assert!(world.asset_input_chunk(request.id, &texture[..first])?);
    let partial = super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
    assert_eq!(partial.draw_calls, 0);
    assert_eq!(partial.uploaded_bytes, row_bytes as u32);
    assert!(world.asset_resources().get(key).unwrap().data().is_none());
    let pending = capture()?;
    assert_eq!(coverage(&pending).0, 0, "private uploads cannot be drawn");
    save(output, "stream-partial", &pending)?;

    // Cancel after real allocation/upload, then reopen the same object. The old
    // reader cannot supply bytes or EOF to this new loader.
    world.asset_resources_mut().unload(key);
    drop(world);
    host.flush_resource_lifecycle();
    world = host.world_mut(world_id).unwrap();
    assert_eq!(world.asset_input_bytes(), 0);
    assert_eq!(
        super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?.draw_calls,
        0
    );
    let replacement = world.take_resource_requests().pop().unwrap();
    assert_ne!(request.id, replacement.id);
    let replacement_input_bytes = world.asset_input_bytes();
    assert!(world.asset_input_chunk(request.id, &[255; 32])?);
    world.asset_input_end(request.id, Ok(()));
    assert_eq!(
        world.asset_input_bytes(),
        replacement_input_bytes,
        "stale input cannot add buffered bytes to the replacement"
    );

    let mut uploaded = 0;
    for chunk in texture.chunks(STREAM_CAPACITY) {
        assert!(world.asset_input_chunk(replacement.id, chunk)?);
        assert!(world.asset_input_bytes() <= STREAM_CAPACITY);
        let stats = super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?;
        assert_eq!(stats.draw_calls, 0, "all bytes still require valid EOF");
        uploaded += stats.uploaded_bytes;
        assert!(world.asset_resources().get(key).unwrap().data().is_none());
    }
    assert_eq!(uploaded as usize, texture.len() - 16);
    assert_eq!(coverage(&capture()?).0, 0);
    world.asset_input_end(replacement.id, Ok(()));
    assert_eq!(
        super::world::present_world!(renderer, host, world, WIDTH, HEIGHT)?.draw_calls,
        1
    );
    let resource = world.asset_resources().get(key).unwrap();
    assert_eq!(resource.status(), &AssetLoadStatus::Loaded);
    assert_eq!(resource.key(), key);
    let loaded = capture()?;
    assert_eq!(loaded, expected);
    save(output, "stream-loaded", &loaded)?;
    Ok(())
}

const ODD_RGB: [[u8; 3]; 6] = [
    [128, 64, 192],
    [255, 0, 0],
    [0, 255, 0],
    [0, 0, 255],
    [0, 0, 0],
    [32, 160, 224],
];

fn checker_rgb(x: u32, y: u32, width: u32, height: u32) -> [u8; 3] {
    let palette = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [0, 0, 0]];
    palette[((u64::from(x) * 8 / u64::from(width) + u64::from(y) * 8 / u64::from(height)) % 4)
        as usize]
}

fn assert_payload(texture: &TextureAsset, source: impl Fn(u32, u32) -> [u8; 3]) {
    assert_eq!(
        texture.pixels().len(),
        (texture.width() * texture.height() * 4) as usize
    );
    for y in 0..texture.height() {
        for x in 0..texture.width() {
            let offset = ((y * texture.width() + x) * 4) as usize;
            assert_eq!(
                &texture.pixels()[offset..offset + 3],
                &source(x, y),
                "RGB texel ({x}, {y})"
            );
            assert_eq!(texture.pixels()[offset + 3], 255, "alpha texel ({x}, {y})");
        }
    }
}

fn texture_component(entity: EntityRef) -> Command {
    Command::InsertComponent {
        entity,
        component: ComponentValue::UNLIT_TEXTURE,
        fields: vec![FieldWrite {
            offset: std::mem::offset_of!(UnlitTexture, source) as u32,
            value: FieldValue::String("fixture:///fixture.texture".into()),
        }],
    }
}

// Independent geometric oracle: intersect camera rays with the fixture's real
// triangles and interpolate its UVs. This checks spatial samples rather than
// merely finding the source colors somewhere in the image. Expected texels come
// from fixture definitions independently of the runtime's retained pixel bytes.
fn assert_samples(
    pixels: &[u8],
    mesh: &MeshAsset,
    texture: &TextureAsset,
    source_rgb: impl Fn(u32, u32) -> [u8; 3],
    minimum_colors: usize,
) {
    let eye = [3.0, 2.0, 5.0];
    let back = normalize(eye);
    let right = normalize([5.0, 0.0, -3.0]);
    let up = cross(back, right);
    let tangent = (std::f64::consts::PI / 8.0).tan();
    let mut tested = 0;
    let mut mismatches = 0;
    let mut face_colors: BTreeMap<usize, BTreeMap<[u8; 3], usize>> = BTreeMap::new();
    for y in (0..HEIGHT as usize).step_by(3) {
        for x in (0..WIDTH as usize).step_by(3) {
            let screen_x =
                (2.0 * (x as f64 + 0.5) / f64::from(WIDTH) - 1.0) * tangent * f64::from(WIDTH)
                    / f64::from(HEIGHT);
            let screen_y = (1.0 - 2.0 * (y as f64 + 0.5) / f64::from(HEIGHT)) * tangent;
            let direction =
                std::array::from_fn(|i| -back[i] + right[i] * screen_x + up[i] * screen_y);
            let mut nearest = f64::INFINITY;
            let mut hit = None;
            for (triangle, indices) in mesh.indices().as_chunks::<3>().0.iter().enumerate() {
                let vertices = indices.map(|index| mesh.positions()[index as usize]);
                let a = std::array::from_fn(|i| f64::from(vertices[0][i]));
                let b = std::array::from_fn(|i| f64::from(vertices[1][i]));
                let c = std::array::from_fn(|i| f64::from(vertices[2][i]));
                let e1 = subtract(b, a);
                let e2 = subtract(c, a);
                let h = cross(direction, e2);
                let determinant = dot(e1, h);
                if determinant <= 1e-10 {
                    continue;
                }
                let s = subtract(eye, a);
                let u = dot(s, h) / determinant;
                let q = cross(s, e1);
                let v = dot(direction, q) / determinant;
                let distance = dot(e2, q) / determinant;
                if u < 0.0 || v < 0.0 || u + v > 1.0 || distance <= 0.0 || distance >= nearest {
                    continue;
                }
                nearest = distance;
                hit = Some((triangle, *indices, [1.0 - u - v, u, v]));
            }
            let Some((triangle, indices, weights)) = hit else {
                continue;
            };
            if weights.iter().any(|weight| *weight < 0.04) {
                continue;
            }
            let uv: [f64; 2] = std::array::from_fn(|axis| {
                indices
                    .iter()
                    .zip(weights)
                    .map(|(index, weight)| {
                        f64::from(mesh.uvs().unwrap()[*index as usize][axis]) * weight
                    })
                    .sum()
            });
            let texel = [
                uv[0].rem_euclid(1.0) * f64::from(texture.width()),
                uv[1].rem_euclid(1.0) * f64::from(texture.height()),
            ];
            // Avoid texel boundaries where independent f32/f64 interpolation
            // and subpixel raster precision can select neighboring texels.
            if texel
                .iter()
                .any(|value| value.fract() < 0.1 || value.fract() > 0.9)
            {
                continue;
            }
            let source = source_rgb(texel[0] as u32, texel[1] as u32);
            let mut expected = [255; 4];
            for channel in 0..3 {
                let encoded = f64::from(source[channel]) / 255.0;
                let sampled = if encoded <= 0.04045 {
                    encoded / 12.92
                } else {
                    ((encoded + 0.055) / 1.055).powf(2.4)
                };
                let vertex: f64 = indices
                    .iter()
                    .zip(weights)
                    .map(|(index, weight)| {
                        f64::from(
                            mesh.colors()
                                .map_or(1.0, |colors| colors[*index as usize][channel]),
                        ) * weight
                    })
                    .sum();
                let contribution: f64 = indices
                    .iter()
                    .zip(weights)
                    .map(|(index, barycentric)| {
                        mesh.texture_weights()
                            .map_or(1.0, |values| f64::from(values[*index as usize]) / 255.0)
                            * barycentric
                    })
                    .sum();
                let linear =
                    (1.0 + (sampled - 1.0) * contribution) * vertex * f64::from(MATERIAL[channel]);
                let output = if linear <= 0.0031308 {
                    linear * 12.92
                } else {
                    1.055 * linear.powf(1.0 / 2.4) - 0.055
                };
                expected[channel] = (output * 255.0).round() as u8;
            }
            let pixel = pixels.as_chunks::<4>().0[y * WIDTH as usize + x];
            tested += 1;
            if !matches(&pixel, &expected) {
                mismatches += 1;
            }
            *face_colors
                .entry(triangle / 2)
                .or_default()
                .entry([source[0], source[1], source[2]])
                .or_default() += 1;
        }
    }
    assert!(tested > 300, "insufficient interior UV samples: {tested}");
    assert!(
        mismatches < tested / 100,
        "UV/color mismatches: {mismatches}/{tested}"
    );
    let alternating_faces = face_colors
        .values()
        .filter(|colors| colors.values().filter(|count| **count > 10).count() >= minimum_colors)
        .count();
    assert!(
        alternating_faces >= 3,
        "expected {minimum_colors} source colors on three faces: {face_colors:?}"
    );
    println!(
        "UV oracle: {tested} samples, {mismatches} mismatches, {alternating_faces} multicolor faces"
    );
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn subtract(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}

fn normalize(a: [f64; 3]) -> [f64; 3] {
    let length = dot(a, a).sqrt();
    a.map(|value| value / length)
}

// The corpus goes through ordinary owned ingress. Wire construction stays in
// this fixture helper; render/capture/rebuild remain environment-independent.
fn optional_payload(mesh: &MeshAsset, color: bool, uv: bool, weights: Option<Vec<u8>>) -> Vec<u8> {
    let mut streams = vec![(
        0u8,
        1u8,
        mesh.positions()
            .iter()
            .flatten()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )];
    if color {
        streams.push((
            1,
            1,
            mesh.colors()
                .unwrap()
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect(),
        ));
    }
    if uv {
        streams.push((
            2,
            2,
            mesh.uvs()
                .unwrap()
                .iter()
                .flatten()
                .flat_map(|v| v.to_le_bytes())
                .collect(),
        ));
    }
    if let Some(weights) = weights {
        streams.push((3, 3, weights));
    }
    let mut bytes = b"IPPM".to_vec();
    for value in [
        3,
        mesh.vertex_count() as u32,
        mesh.indices().len() as u32,
        streams.len() as u32,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    for (semantic, format, data) in &streams {
        bytes.extend([*semantic, *format, 0, 0]);
        bytes.extend((data.len() as u32).to_le_bytes());
    }
    for (_, _, data) in streams {
        bytes.extend(data);
    }
    bytes.extend(mesh.indices().iter().flat_map(|v| v.to_le_bytes()));
    bytes
}

fn optional_streams<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mesh: Vec<u8>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    rebuild: &mut impl FnMut() -> Result<D>,
    output: &Path,
) -> Result<()> {
    // Mixed, asymmetric channels exercise sRGB decode, channel order and
    // exercise sRGB decode, channel order and top-left row order independently.
    let mut texture = b"IPPT".to_vec();
    for value in [3u32, 3, 2] {
        texture.extend(value.to_le_bytes());
    }
    texture.extend(
        ODD_RGB
            .into_iter()
            .flat_map(|rgb| rgb.into_iter().chain([255])),
    );
    std::fs::write(output.join("odd-rgb.texture"), &texture)?;
    let decoded_texture = TextureAsset::decode(&texture)?.0;
    let odd_source = |x, y| ODD_RGB[(y * 3 + x) as usize];

    let mut source_host = ipp_core::HostRuntime::new();
    let source_id = source_host.create_world(Default::default())?;
    let mut source = source_host.world_mut(source_id).unwrap();
    source
        .enqueue_mesh(MeshUpload {
            id: 1,
            key: KEY,
            bytes: mesh,
        })
        .map_err(|e| format!("optional source: {e:?}"))?;
    source.prepare_update(0.0)?;
    source.poll_all_assets();
    source
        .step(0.0)
        .map_err(|e| format!("optional boundary: {e:?}"))?;
    let mesh = source.mesh(KEY).unwrap();
    let n = mesh.vertex_count();
    let cases = [
        ("position", false, false, None),
        ("color", true, false, None),
        ("uv-defaults", false, true, None),
        ("uv-color", true, true, None),
        ("weight-zero", false, true, Some(vec![0; n])),
        ("weight-half", true, true, Some(vec![128; n])),
        ("weight-full", false, true, Some(vec![255; n])),
        (
            "weight-varying",
            true,
            true,
            Some((0..n).map(|i| [0, 85, 170, 255][i % 4]).collect()),
        ),
        ("position-after-weight", false, false, None),
    ];
    renderer.replace_device(&mut ipp_core::HostRuntime::new(), rebuild()?)?;
    assert_eq!(
        renderer.cached_program_count(),
        0,
        "no eager shader compilation"
    );
    super::world::render_frame(
        renderer,
        &mut super::world::empty_world(&mut ipp_core::HostRuntime::new()),
        WIDTH,
        HEIGHT,
    )?;
    assert_eq!(
        renderer.cached_program_count(),
        0,
        "empty world needs no program"
    );
    let mut solid = None;
    let mut textured = None;
    let mut evidence = String::from("layout,vertex_bytes,upload_bytes,programs\n");
    for (name, color, uv, weights) in cases {
        super::world::render_frame(
            renderer,
            &mut super::world::empty_world(&mut ipp_core::HostRuntime::new()),
            WIDTH,
            HEIGHT,
        )?;
        let vertex_bytes = n
            * (12 + usize::from(color) * 12 + usize::from(uv) * 8 + usize::from(weights.is_some()));
        let bytes = optional_payload(mesh, color, uv, weights.clone());
        let mut world_host = ipp_core::HostRuntime::new();
        renderer.install(&mut world_host)?;
        let mut world = super::world::fixture_world(&mut world_host)?;
        let mut commands = vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::TRANSFORM,
                fields: vec![],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::UNLIT_MATERIAL,
                fields: vec![
                    float(std::mem::offset_of!(UnlitMaterial, r), MATERIAL[0]),
                    float(std::mem::offset_of!(UnlitMaterial, g), MATERIAL[1]),
                    float(std::mem::offset_of!(UnlitMaterial, b), MATERIAL[2]),
                ],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: ComponentValue::MESH_INSTANCE,
                fields: vec![FieldWrite {
                    offset: std::mem::offset_of!(MeshInstance, source) as u32,
                    value: FieldValue::String("fixture:///fixture.mesh".into()),
                }],
            },
        ];
        if uv {
            commands.push(texture_component(EntityRef::Alias(1)));
        }
        apply(&mut world, commands)?;
        assert!(world.render_items().is_empty());
        let stats = deliver!(renderer, world_host, world, &bytes, Some(&texture))?;
        assert!(world.mesh(world.render_items()[0].mesh).is_none());
        let decoded_mesh = MeshAsset::decode(&bytes)?.0;
        assert_eq!(decoded_mesh.vertex_bytes(), vertex_bytes);
        if uv {
            assert_payload(&decoded_texture, odd_source);
        }
        let expected_upload = vertex_bytes
            + std::mem::size_of_val(mesh.indices())
            + if uv {
                decoded_texture.pixels().len()
            } else {
                0
            };
        assert_eq!(
            stats.uploaded_bytes as usize, expected_upload,
            "{name}: only present data uploads"
        );
        assert_eq!(
            renderer.cached_program_count(),
            1,
            "only the current World's program demand stays resident"
        );
        let pixels = capture()?;
        save(output, &format!("optional-{name}"), &pixels)?;
        if !uv {
            if let Some(reference) = &solid {
                assert_eq!(
                    &pixels, reference,
                    "absent color is white after other draws"
                );
            } else {
                solid = Some(pixels.clone());
            }
        } else {
            assert_samples(&pixels, &decoded_mesh, &decoded_texture, odd_source, 2);
            if weights.is_none() {
                if let Some(reference) = &textured {
                    assert_eq!(
                        &pixels, reference,
                        "optional color does not change texture shading"
                    );
                } else {
                    textured = Some(pixels.clone());
                }
            } else if name == "weight-zero" {
                assert_eq!(
                    &pixels,
                    solid.as_ref().unwrap(),
                    "zero contribution preserves material tint"
                );
            } else if name == "weight-full" {
                assert_eq!(
                    &pixels,
                    textured.as_ref().unwrap(),
                    "absent weight equals normalized byte 255"
                );
            }
        }
        assert_eq!(
            super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?
                .uploaded_bytes,
            0
        );
        evidence.push_str(&format!(
            "{name},{vertex_bytes},{expected_upload},{}\n",
            renderer.cached_program_count()
        ));
        if name == "weight-varying" {
            let identity = world.render_items()[0].mesh;
            let world_id = world.id();
            drop(world);
            renderer.replace_device(&mut world_host, rebuild()?)?;
            world = world_host.world_mut(world_id).unwrap();
            assert_eq!(
                world
                    .resource_snapshots()
                    .iter()
                    .find(|asset| asset.kind == ipp_core::MESH_TYPE)
                    .unwrap()
                    .id,
                identity.asset
            );
            assert_eq!(renderer.cached_program_count(), 0);
            assert_eq!(
                deliver!(renderer, world_host, world, &bytes, Some(&texture))?.uploaded_bytes
                    as usize,
                expected_upload
            );
            assert_eq!(
                renderer.cached_program_count(),
                1,
                "rebuild only current demand"
            );
            assert_eq!(
                capture()?,
                pixels,
                "weighted retained streams rebuild exactly"
            );
            // Add untextured demand for the final alternating layout.
            let entity = EntityRef::Handle(world.render_items()[0].entity);
            apply(
                &mut world,
                vec![Command::RemoveComponent {
                    entity,
                    component: ComponentValue::UNLIT_TEXTURE,
                }],
            )?;
            super::world::present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
            drop(world);
            world_host.flush_resource_lifecycle();
            assert_eq!(renderer.cached_program_count(), 1);
        }
    }
    std::fs::write(output.join("optional-streams.csv"), evidence)?;
    Ok(())
}
