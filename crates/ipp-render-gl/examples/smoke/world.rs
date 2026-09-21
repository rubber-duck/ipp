use std::path::Path;

use ipp_core::{
    Batch, Command, ComponentValue, EntityRef, FieldValue, FieldWrite, WorldContext,
    components::{MeshInstance, Transform, UnlitMaterial},
};
use ipp_render_gl::{RenderDevice, RenderService};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub const WIDTH: u32 = 320;
pub const HEIGHT: u32 = 240;
pub(crate) const KEY: ipp_core::MeshKey = ipp_core::MeshKey {
    asset: 1,

    variant: 0,
};
pub(crate) const MATERIAL: [f32; 3] = [0.25, 0.5, 0.75];
const BACKGROUND: [u8; 4] = [10, 14, 20, 255];

/// Install only the fixture provider implemented by this native test host.
pub(crate) fn fixture_world(host: &mut ipp_core::HostRuntime) -> Result<WorldContext<'_>> {
    if host.data_sources().registration_id("fixture:///").is_none() {
        host.data_sources_mut().register_stream("fixture://")?;
    }
    let id = host.create_world(Default::default())?;
    let mut world = host.world_mut(id).unwrap();

    let yaw = 3.0_f32.atan2(5.0) * 0.5;
    let pitch = -2.0_f32.atan2(34.0_f32.sqrt()) * 0.5;
    apply(
        &mut world,
        vec![
            Command::Create {
                alias: 0,
                metadata: ipp_core::EntityMetadata {
                    symbolic_id: Some("fixture-camera".into()),
                    classes: vec!["fixture-camera".into()],
                },
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(0),
                component: ComponentValue::TRANSFORM,
                fields: vec![
                    float(std::mem::offset_of!(Transform, x), 3.0),
                    float(std::mem::offset_of!(Transform, y), 2.0),
                    float(std::mem::offset_of!(Transform, z), 5.0),
                    float(std::mem::offset_of!(Transform, qx), pitch.sin() * yaw.cos()),
                    float(std::mem::offset_of!(Transform, qy), pitch.cos() * yaw.sin()),
                    float(
                        std::mem::offset_of!(Transform, qz),
                        -pitch.sin() * yaw.sin(),
                    ),
                    float(std::mem::offset_of!(Transform, qw), pitch.cos() * yaw.cos()),
                ],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(0),
                component: ComponentValue::CAMERA,
                fields: vec![],
            },
        ],
    )?;
    let camera = world.entities()[0].id;
    world.enqueue_camera_activate(camera)?;
    world.step(0.0)?;
    assert_eq!(world.active_camera(), Some(camera));

    Ok(world)
}

// Scenario intent is independent of EGL/process setup: a different native host
// can supply its device and completed-frame capture using the same cube bytes.
pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    fixture: Vec<u8>,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let mut world_host = ipp_core::HostRuntime::new();
    renderer.install(&mut world_host)?;
    let mut world = fixture_world(&mut world_host)?;
    let transform = ComponentValue::TRANSFORM;
    let material = ComponentValue::UNLIT_MATERIAL;
    let mesh = ComponentValue::MESH_INSTANCE;
    apply(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: transform,
                fields: vec![],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: material,
                fields: vec![
                    float(std::mem::offset_of!(UnlitMaterial, r), MATERIAL[0]),
                    float(std::mem::offset_of!(UnlitMaterial, g), MATERIAL[1]),
                    float(std::mem::offset_of!(UnlitMaterial, b), MATERIAL[2]),
                ],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(1),
                component: mesh,
                fields: vec![FieldWrite {
                    offset: std::mem::offset_of!(MeshInstance, source) as u32,
                    value: FieldValue::String("fixture:///fixture.mesh".into()),
                }],
            },
        ],
    )?;
    assert!(
        world.render_items().is_empty(),
        "world commits before fixture delivery"
    );
    assert_eq!(
        present_world!(renderer, world_host, world, WIDTH, HEIGHT)?.draw_calls,
        0
    );
    let pending = capture()?;
    save(output, "pending", &pending)?;
    assert_eq!(coverage(&pending).0, 0);
    let stats = deliver!(renderer, world_host, world, &fixture, None)?;
    let key = world.render_items()[0].mesh;
    assert!(
        world.mesh(key).is_none(),
        "GPU resources discard bulk CPU streams"
    );
    let mesh_asset = ipp_core::MeshAsset::decode(&fixture)?.0;
    assert_eq!(
        (mesh_asset.vertex_count(), mesh_asset.indices().len()),
        (24, 36)
    );
    let uploaded_bytes =
        (mesh_asset.vertex_bytes() + std::mem::size_of_val(mesh_asset.indices())) as u32;
    let colors: Vec<_> = mesh_asset
        .colors()
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|face| face[0])
        .collect();
    let entity = EntityRef::Handle(world.render_items()[0].entity);

    assert_eq!(
        (stats.draw_calls, stats.triangles, stats.uploaded_bytes),
        (1, 12, uploaded_bytes)
    );
    let visible = capture()?;
    save(output, "visible", &visible)?;
    let initial = coverage(&visible);
    assert!(initial.0 > 3000, "cube coverage too small: {}", initial.0);
    assert!(
        initial.0 < (WIDTH * HEIGHT / 2) as usize,
        "cube filled implausibly much of viewport"
    );
    assert_colors(&visible, &colors, MATERIAL);
    assert_eq!(
        present_world!(renderer, world_host, world, WIDTH, HEIGHT)?.uploaded_bytes,
        0,
        "unchanged immutable mesh must remain resident"
    );

    // A second committed item remains pending while the first frame is intact.
    // Host delivery alone then changes the image, with no world resubmission.
    apply(
        &mut world,
        vec![
            Command::Create {
                alias: 2,
                metadata: Default::default(),
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(2),
                component: transform,
                fields: vec![
                    float(std::mem::offset_of!(Transform, x), -1.7),
                    float(std::mem::offset_of!(Transform, sx), 0.5),
                    float(std::mem::offset_of!(Transform, sy), 0.5),
                    float(std::mem::offset_of!(Transform, sz), 0.5),
                ],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(2),
                component: material,
                fields: vec![],
            },
            Command::InsertComponent {
                entity: EntityRef::Alias(2),
                component: mesh,
                fields: vec![FieldWrite {
                    offset: std::mem::offset_of!(MeshInstance, source) as u32,
                    value: FieldValue::String("fixture:///delayed.mesh".into()),
                }],
            },
        ],
    )?;
    assert_eq!(
        present_world!(renderer, world_host, world, WIDTH, HEIGHT)?.draw_calls,
        1
    );
    let unaffected = capture()?;
    save(output, "pending-unaffected", &unaffected)?;
    assert_eq!(
        unaffected, visible,
        "pending geometry leaves ready geometry visible"
    );
    let delayed = world
        .entities()
        .into_iter()
        .find(|snapshot| {
            EntityRef::Handle(snapshot.id) != entity && Some(snapshot.id) != world.active_camera()
        })
        .unwrap()
        .id;
    let request = world.take_resource_requests();
    assert_eq!(request.len(), 1);
    world.complete_resource(request[0].id, Ok(fixture.clone()))?;
    assert_eq!(world.render_items().len(), 1);
    world.step(0.0)?;
    assert_eq!(
        present_world!(renderer, world_host, world, WIDTH, HEIGHT)?.draw_calls,
        2
    );
    let arrived = capture()?;
    save(output, "pending-arrived", &arrived)?;
    assert!(
        arrived
            .as_chunks::<4>()
            .0
            .iter()
            .zip(visible.as_chunks::<4>().0)
            .filter(|(a, b)| a != b)
            .count()
            > 500
    );
    apply(
        &mut world,
        vec![Command::Delete {
            entity: EntityRef::Handle(delayed),
        }],
    )?;
    assert_eq!(
        present_world!(renderer, world_host, world, WIDTH, HEIGHT)?.draw_calls,
        1
    );
    assert_eq!(capture()?, visible);

    apply(
        &mut world,
        vec![Command::SetField {
            entity,
            component: transform,
            field: float(std::mem::offset_of!(Transform, x), 1.0),
        }],
    )?;
    assert_eq!(
        present_world!(renderer, world_host, world, WIDTH, HEIGHT)?.uploaded_bytes,
        0
    );
    let moved = capture()?;
    save(output, "moved", &moved)?;
    let moved_coverage = coverage(&moved);
    assert!(moved_coverage.0 > 3000);
    assert!(
        moved_coverage.1 > initial.1 + 15.0,
        "translation must move image right: {} -> {}",
        initial.1,
        moved_coverage.1
    );
    let difference = visible
        .as_chunks::<4>()
        .0
        .iter()
        .zip(moved.as_chunks::<4>().0.iter())
        .filter(|(a, b)| a != b)
        .count();
    assert!(
        difference > 2000,
        "translation must change meaningful image regions"
    );

    // Exercise the low segment of the exact sRGB transfer function as well as
    // ordinary midrange color; this catches omitted and double conversion.
    apply(
        &mut world,
        vec![Command::SetField {
            entity,
            component: material,
            field: float(std::mem::offset_of!(UnlitMaterial, r), 0.001),
        }],
    )?;
    present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
    let low = capture()?;
    save(output, "low-linear", &low)?;
    assert_colors(&low, &colors, [0.001, MATERIAL[1], MATERIAL[2]]);

    // Keep position-only rendering in the lean world harness as well as the
    // expanded texture corpus. The upload fixture owns exact V3 wire bytes.
    let source = &mesh_asset;
    let mut bytes = b"IPPM".to_vec();
    for value in [
        3,
        source.vertex_count() as u32,
        source.indices().len() as u32,
        1,
    ] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([0, 1, 0, 0]);
    bytes.extend((std::mem::size_of_val(source.positions()) as u32).to_le_bytes());
    bytes.extend(
        source
            .positions()
            .iter()
            .flatten()
            .flat_map(|value| value.to_le_bytes()),
    );
    bytes.extend(
        source
            .indices()
            .iter()
            .flat_map(|value| value.to_le_bytes()),
    );
    apply(
        &mut world,
        vec![Command::SetField {
            entity,
            component: mesh,
            field: FieldWrite {
                offset: std::mem::offset_of!(MeshInstance, source) as u32,
                value: FieldValue::String("fixture:///position.mesh".into()),
            },
        }],
    )?;
    assert!(world.render_items().is_empty());
    let stats = deliver!(renderer, world_host, world, &bytes, None)?;
    assert_eq!(stats.uploaded_bytes, 24 * 12 + 36 * 2);
    let position_only = capture()?;
    save(output, "position-only-lean", &position_only)?;
    assert_colors(
        &position_only,
        &[[1.0; 3]],
        [0.001, MATERIAL[1], MATERIAL[2]],
    );
    assert_eq!(
        renderer.cached_program_count(),
        1,
        "layout does not add shader behavior"
    );

    apply(
        &mut world,
        vec![Command::Delete {
            entity,
        }],
    )?;
    let stats = present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
    assert_eq!(stats.draw_calls, 0);
    let removed = capture()?;
    save(output, "removed", &removed)?;
    assert_eq!(
        coverage(&removed).0,
        0,
        "removed world must contain only the clear color"
    );
    // WorldContext contracts retain debug declarations even in a renderer without
    // private debug assets; no provider requests or resource events are created.
    apply(
        &mut world,
        vec![
            Command::Create {
                alias: 1,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::Transform(Transform::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(1),
                value: ComponentValue::BoundingGeometry(ipp_core::components::BoundingGeometry {
                    geometry: ipp_core::systems::geometry::GeometryDefinition::from(
                        ipp_core::systems::geometry::GeometryShape::default(),
                    )
                    .encode()?,
                    ..Default::default()
                }),
            },
        ],
    )?;
    let patch = ipp_core::RenderStatePatch {
        show_all_debug_geometries: Some(true),
        ..Default::default()
    };
    world.enqueue_render_state_update(patch)?;
    let report = world.step(0.0)?;
    assert_eq!(report.render_state_changes[0].changes, patch);
    assert!(report.resource_changes.is_empty());
    assert!(world.take_resource_requests().is_empty());
    assert!(world.resource_snapshots().is_empty());
    let stats = present_world!(renderer, world_host, world, WIDTH, HEIGHT)?;
    assert_eq!(stats.draw_calls, 1);
    assert!(coverage(&capture()?).0 > 0);
    drop(world);
    Ok(())
}

/// A bounded fixture provider for the native test host. The scenario selects
/// sources first; this runner delivers owned bytes through the production API.
pub(crate) fn deliver_to_host<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut ipp_core::HostRuntime,
    id: ipp_core::WorldId,
    mesh: &[u8],
    texture: Option<&[u8]>,
) -> Result<ipp_render_gl::RenderStats> {
    let _ = texture;
    let mut uploaded = 0u32;
    for _ in 0..512 {
        let stats = render_host_frame(renderer, host, id, WIDTH, HEIGHT)?;
        uploaded = uploaded.saturating_add(stats.uploaded_bytes);
        let mut world = host.world_mut(id).unwrap();
        for id in world.take_resource_cancellations() {
            let _ = id;
        }
        for request in world.take_resource_requests() {
            let bytes = match request.kind {
                ipp_core::AssetResourceKind::Mesh => mesh,
                ipp_core::AssetResourceKind::Texture => texture.ok_or("missing texture fixture")?,
                _ => return Err("unexpected fixture asset type".into()),
            };
            world.complete_resource(request.id, Ok(bytes.to_vec()))?;
        }
        let snapshots = world.resource_snapshots();
        for snapshot in &snapshots {
            if let ipp_core::AssetResourceStatus::Failed(error) = &snapshot.status {
                return Err(error.clone().into());
            }
        }
        if snapshots
            .iter()
            .all(|snapshot| snapshot.status == ipp_core::AssetResourceStatus::Loaded)
        {
            // Readiness may have changed while delivering input after the capture.
            drop(world);
            let mut stats = render_host_frame(renderer, host, id, WIDTH, HEIGHT)?;
            uploaded = uploaded.saturating_add(stats.uploaded_bytes);
            stats.uploaded_bytes = uploaded;
            return Ok(stats);
        }
        world.step(0.0)?;
    }
    Err("asset readiness deadline exhausted".into())
}

macro_rules! deliver {
    ($renderer:expr, $host:ident, $world:ident, $mesh:expr, $texture:expr) => {{
        let id = $world.id();
        drop($world);
        let result =
            $crate::smoke::world::deliver_to_host($renderer, &mut $host, id, $mesh, $texture);
        $world = $host.world_mut(id).unwrap();
        result
    }};
}

pub(crate) use deliver;

pub(crate) fn float(offset: usize, value: f32) -> FieldWrite {
    FieldWrite {
        offset: offset as u32,
        value: FieldValue::F32(value),
    }
}

pub(crate) fn apply(world: &mut WorldContext<'_>, operations: Vec<Command>) -> Result<()> {
    world
        .enqueue(Batch {
            id: world.tick() + 1,
            operations,
        })
        .map_err(|e| format!("enqueue: {e:?}"))?;
    let report = world.step(0.0).map_err(|e| format!("boundary: {e:?}"))?;
    for outcome in report.outcomes {
        outcome
            .result
            .map_err(|e| format!("mutation rejected: {e:?}"))?;
    }
    Ok(())
}

pub(crate) fn matches(pixel: &[u8; 4], expected: &[u8; 4]) -> bool {
    pixel.iter().zip(expected).all(|(a, b)| a.abs_diff(*b) <= 2)
}

pub(crate) fn coverage(pixels: &[u8]) -> (usize, f64) {
    assert_eq!(pixels.len(), (WIDTH * HEIGHT * 4) as usize);
    let mut count = 0;
    let mut sum_x = 0;
    for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
        if !matches(pixel, &BACKGROUND) {
            count += 1;
            sum_x += index % WIDTH as usize;
        }
    }
    (count, sum_x as f64 / count.max(1) as f64)
}

fn assert_colors(pixels: &[u8], colors: &[[f32; 3]], material: [f32; 3]) {
    let expected: Vec<[u8; 4]> = colors
        .iter()
        .map(|color| {
            let mut rgba = [255; 4];
            for channel in 0..3 {
                let linear = f64::from(color[channel]) * f64::from(material[channel]);
                let encoded = if linear <= 0.0031308 {
                    12.92 * linear
                } else {
                    1.055 * linear.powf(1.0 / 2.4) - 0.055
                };
                rgba[channel] = (encoded * 255.0).round() as u8;
            }
            rgba
        })
        .collect();
    let matching = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| expected.iter().any(|color| matches(pixel, color)))
        .count();
    let visible = coverage(pixels).0;
    assert!(
        matching > visible * 95 / 100,
        "sRGB face regions mismatch: {matching}/{visible}; expected {expected:?}"
    );
    let visible_faces = expected
        .iter()
        .filter(|color| {
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|pixel| matches(pixel, color))
                .count()
                > 100
        })
        .count();
    assert!(
        visible_faces >= colors.len().min(3),
        "insufficient visible unlit cube color regions: {visible_faces}"
    );
}

pub(crate) fn save(output: &Path, name: &str, pixels: &[u8]) -> Result<()> {
    let mut ppm = format!("P6\n{WIDTH} {HEIGHT}\n255\n").into_bytes();
    for pixel in pixels.as_chunks::<4>().0.iter() {
        ppm.extend_from_slice(&pixel[..3]);
    }
    std::fs::write(output.join(format!("{name}.ppm")), ppm)?;
    Ok(())
}

/// Empty world for clearing a render target without retaining another fixture.
pub(crate) fn empty_world(host: &mut ipp_core::HostRuntime) -> WorldContext<'_> {
    let id = host.create_world(Default::default()).unwrap();
    host.world_mut(id).unwrap()
}

/// The embedding host progresses resources and evaluates before presentation.
pub(crate) fn render_frame<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    world: &mut WorldContext<'_>,
    width: u32,
    height: u32,
) -> std::result::Result<ipp_render_gl::RenderStats, String> {
    renderer.begin_frame();
    let mut uploaded = 0u32;
    // New built-in recipes are registered during demand collection. Complete
    // their next Host loading phase before callers inspect a completed frame.
    for _ in 0..8 {
        world
            .prepare_update(0.0)
            .map_err(|error| error.to_string())?;
        world.poll_all_assets();
        world.step(0.0).map_err(|error| error.to_string())?;
        let mut stats = renderer
            .render(world, width, height)
            .map_err(|error| error.to_string())?;
        uploaded = uploaded.saturating_add(stats.uploaded_bytes);
        if !world.asset_resources().iter().any(|resource| {
            resource.source().uri.starts_with("ipp-render://program/")
                && matches!(
                    resource.status(),
                    ipp_core::services::asset_management::AssetLoadStatus::Unloaded
                )
        }) {
            stats.uploaded_bytes = uploaded;
            return Ok(stats);
        }
    }
    Err("Built-in program preparation did not settle".into())
}

// Fixture code can keep its convenient World borrow between presentations. A
// presentation releases that borrow so the Host can notify every resource user.
macro_rules! present_world {
    ($renderer:expr, $host:ident, $world:ident, $width:expr, $height:expr) => {{
        let id = $world.id();
        drop($world);
        let result =
            $crate::smoke::world::render_host_frame($renderer, &mut $host, id, $width, $height);
        $world = $host.world_mut(id).unwrap();
        result
    }};
}

pub(crate) use present_world;

/// Host-driven readiness and presentation, including all resource lifecycle recipients.
pub(crate) fn render_host_frame<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut ipp_core::HostRuntime,
    id: ipp_core::WorldId,
    width: u32,
    height: u32,
) -> std::result::Result<ipp_render_gl::RenderStats, String> {
    renderer.begin_frame();
    let mut uploaded = 0u32;
    for _ in 0..8 {
        host.world_mut(id)
            .unwrap()
            .prepare_update(0.0)
            .map_err(|error| error.to_string())?;
        host.progress_assets();
        let mut world = host.world_mut(id).unwrap();
        world.step(0.0).map_err(|error| error.to_string())?;
        let mut stats = renderer
            .render(&mut world, width, height)
            .map_err(|error| error.to_string())?;
        uploaded = uploaded.saturating_add(stats.uploaded_bytes);
        if !world.asset_resources().iter().any(|resource| {
            resource.source().uri.starts_with("ipp-render://program/")
                && matches!(
                    resource.status(),
                    ipp_core::services::asset_management::AssetLoadStatus::Unloaded
                )
        }) {
            stats.uploaded_bytes = uploaded;
            return Ok(stats);
        }
    }
    Err("Built-in program preparation did not settle".into())
}

/// Two worlds share immutable resources and one render service while retaining
/// their own camera, transforms and lifetime. Capture through the real device.
pub fn multiple_worlds<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mesh: &[u8],
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let mut host = ipp_core::HostRuntime::new();
    renderer.install(&mut host)?;
    let mut worlds = Vec::new();
    for x in [-1.5, 1.5] {
        let mut world = fixture_world(&mut host)?;
        apply(
            &mut world,
            vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::Transform(Transform {
                        x,
                        ..Default::default()
                    }),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::MeshInstance(MeshInstance {
                        source: "fixture:///shared.mesh".into(),
                        variant: 0,
                    }),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::UnlitMaterial(UnlitMaterial::default()),
                },
            ],
        )?;
        worlds.push(world.id());
    }
    host.progress_assets();
    let requests = host.take_resource_requests();
    assert_eq!(requests.len(), 1, "one acquisition for both worlds");
    host.complete_resource(requests[0].id, Ok(mesh.to_vec()))?;
    for _ in 0..16 {
        host.progress_assets();
        for &id in &worlds {
            host.world_mut(id).unwrap().step(0.0)?;
        }
        if host.world_mut(worlds[0]).unwrap().render_items().len() == 1 {
            break;
        }
    }
    let resource = host.world_mut(worlds[0]).unwrap().resource_snapshots()[0].id;
    assert_eq!(
        host.world_mut(worlds[1]).unwrap().resource_snapshots()[0].id,
        resource
    );
    let mut captures = Vec::new();
    // Collect both Worlds' program demand, then run the shared Host loading phase.
    for &id in &worlds {
        renderer.render(&mut host.world_mut(id).unwrap(), WIDTH, HEIGHT)?;
    }
    host.progress_assets();
    for &id in &worlds {
        host.world_mut(id).unwrap().step(0.0)?;
    }
    renderer.begin_frame();
    for (index, &id) in worlds.iter().enumerate() {
        let stats = renderer.render(&mut host.world_mut(id).unwrap(), WIDTH, HEIGHT)?;
        assert_eq!(stats.draw_calls, 1);
        if index == 1 {
            assert_eq!(
                stats.uploaded_bytes, 0,
                "shared GPU upload is accounted once"
            );
        }
        let pixels = capture()?;
        assert!(coverage(&pixels).0 > 1000);
        save(output, &format!("world-{index}"), &pixels)?;
        captures.push(pixels);
    }
    assert_ne!(
        captures[0], captures[1],
        "independent world transforms must produce distinct frames"
    );
    host.destroy_world(worlds[0]);
    host.progress_assets();
    let mut surviving = host.world_mut(worlds[1]).unwrap();
    surviving.step(0.0)?;
    assert_eq!(surviving.resource_snapshots()[0].id, resource);
    renderer.begin_frame();
    assert_eq!(
        renderer
            .render(&mut surviving, WIDTH, HEIGHT)?
            .uploaded_bytes,
        0
    );
    let preserved = capture()?;
    save(output, "world-survives-peer-destruction", &preserved)?;
    assert_eq!(preserved, captures[1]);
    Ok(())
}
