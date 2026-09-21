//! Real framebuffer assertions for debug shading and global visibility overrides.

use std::path::Path;

use ipp_core::{
    Command, ComponentValue, EntityRef, RenderStatePatch, WorldContext,
    components::{BoundingGeometry, Transform},
    systems::geometry::{GeometryDefinition, GeometryShape},
};
use ipp_render_gl::{RenderDevice, RenderService};

use super::world::{HEIGHT, WIDTH, apply, coverage, matches, save};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    for shape in 0..3 {
        for outline in [false, true] {
            let mut world_host = ipp_core::HostRuntime::new();
            let mut world = self::world(&mut world_host, renderer)?;
            add(
                &mut world,
                0,
                Transform::default(),
                BoundingGeometry {
                    geometry: geometry(shape, 2.0),
                    outline,
                    is_rendered: true,
                    ..unit_geometry()
                },
            )?;
            ready(renderer, &mut world)?;
            let pixels = capture()?;
            save(output, &format!("debug-{shape}-{outline}"), &pixels)?;
            let visible = coverage(&pixels).0;
            assert!(visible > 50, "shape {shape} outline {outline} missing");
            assert!(
                count(&pixels, [255, 231, 0, 255]) > visible * 98 / 100,
                "debug color must be uniform, including grey plane vertices"
            );
        }
    }

    let mut world_host = ipp_core::HostRuntime::new();
    let mut world = self::world(&mut world_host, renderer)?;
    add(
        &mut world,
        0,
        Transform {
            x: -1.2,
            sx: 0.8,
            sy: 0.8,
            sz: 0.8,
            ..Transform::default()
        },
        BoundingGeometry {
            geometry: geometry(3, 2.0),
            ..unit_geometry()
        },
    )?;
    add(
        &mut world,
        0,
        Transform {
            x: 1.2,
            sx: 0.8,
            sy: 0.8,
            sz: 0.8,
            ..Transform::default()
        },
        BoundingGeometry {
            geometry: geometry(1, 2.0),
            outline: true,
            is_rendered: true,
            has_color_override: true,
            r: 0.0,
            g: 0.0,
            b: 1.0,
            ..unit_geometry()
        },
    )?;
    ready(renderer, &mut world)?;
    let selected = capture()?;
    save(output, "debug-selected", &selected)?;
    assert!(count(&selected, [0, 0, 255, 255]) > 100);
    assert_eq!(count(&selected, [255, 231, 0, 255]), 0);
    let components = world.entities();
    let resources = world.resource_snapshots();

    update(
        &mut world,
        RenderStatePatch {
            show_all_debug_geometries: Some(true),
            ..RenderStatePatch::default()
        },
    )?;
    assert_eq!(
        super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?.draw_calls,
        2
    );
    let all = capture()?;
    save(output, "debug-all", &all)?;
    assert!(count(&all, [255, 231, 0, 255]) > 500);
    assert_eq!(
        count(&all, [0, 0, 255, 255]),
        count(&selected, [0, 0, 255, 255])
    );
    update(
        &mut world,
        RenderStatePatch {
            debug_geometry_color: Some([1.0, 0.0, 0.0]),
            ..RenderStatePatch::default()
        },
    )?;
    super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
    let red = capture()?;
    save(output, "debug-global-red", &red)?;
    assert!(count(&red, [255, 0, 0, 255]) > 500);
    assert_eq!(
        count(&red, [0, 0, 255, 255]),
        count(&selected, [0, 0, 255, 255])
    );

    update(
        &mut world,
        RenderStatePatch {
            show_all_debug_geometries: Some(false),
            ..RenderStatePatch::default()
        },
    )?;
    super::world::render_frame(renderer, &mut world, WIDTH, HEIGHT)?;
    assert_eq!(
        capture()?,
        selected,
        "turning off global visibility restores the selected subset"
    );
    assert_eq!(world.entities(), components);
    assert_eq!(world.resource_snapshots(), resources);

    // Draw the farther plane last: depth testing must preserve the nearer cube.
    let mut depth_world_host = ipp_core::HostRuntime::new();
    let mut depth_world = self::world(&mut depth_world_host, renderer)?;
    add(
        &mut depth_world,
        0,
        Transform {
            z: 1.0,
            ..Transform::default()
        },
        BoundingGeometry {
            is_rendered: true,
            has_color_override: true,
            r: 0.0,
            g: 0.0,
            b: 1.0,
            ..unit_geometry()
        },
    )?;
    add(
        &mut depth_world,
        0,
        Transform::default(),
        BoundingGeometry {
            geometry: geometry(3, 2.0),
            is_rendered: true,
            ..unit_geometry()
        },
    )?;
    ready(renderer, &mut depth_world)?;
    let depth = capture()?;
    save(output, "debug-depth", &depth)?;
    assert!(count(&depth, [0, 0, 255, 255]) > 3000);
    assert_eq!(
        count(&depth, [255, 231, 0, 255]),
        0,
        "debug geometry must obey ordinary depth testing"
    );
    // The declared pill passes cheap dimensional validation but cannot retain
    // distinct f32 cap latitudes. Its private failure must leave other draws intact.
    add(
        &mut depth_world,
        0,
        Transform::default(),
        BoundingGeometry {
            geometry: geometry(2, 1.0e30),
            is_rendered: true,
            ..unit_geometry()
        },
    )?;
    let failed = super::world::render_frame(renderer, &mut depth_world, WIDTH, HEIGHT)?;
    assert_eq!(failed.draw_calls, 2);
    assert_eq!(failed.failed_draw_calls, 1);
    assert_eq!(capture()?, depth);
    let repeated = super::world::render_frame(renderer, &mut depth_world, WIDTH, HEIGHT)?;
    assert_eq!(repeated.failed_draw_calls, 1);
    assert_eq!(repeated.uploaded_bytes, 0);
    assert!(depth_world.resource_snapshots().is_empty());

    // Identical recipes share private GPU storage; removing demand releases it.
    let mut sharing_host = ipp_core::HostRuntime::new();
    let mut sharing = self::world(&mut sharing_host, renderer)?;
    add(
        &mut sharing,
        0,
        Transform::default(),
        BoundingGeometry {
            is_rendered: true,
            ..unit_geometry()
        },
    )?;
    let single = super::world::render_frame(renderer, &mut sharing, WIDTH, HEIGHT)?;
    add(
        &mut sharing,
        0,
        Transform {
            x: 2.0,
            ..Transform::default()
        },
        BoundingGeometry {
            is_rendered: true,
            ..unit_geometry()
        },
    )?;
    let shared = super::world::render_frame(renderer, &mut sharing, WIDTH, HEIGHT)?;
    assert_eq!(shared.draw_calls, 2);
    assert_eq!(shared.debug_resident_bytes, single.debug_resident_bytes);
    assert_eq!(shared.uploaded_bytes, 0);
    let empty = super::world::render_frame(
        renderer,
        &mut super::world::empty_world(&mut ipp_core::HostRuntime::new()),
        WIDTH,
        HEIGHT,
    )?;
    assert_eq!(empty.debug_resident_bytes, 0);
    let mut bounded_host = ipp_core::HostRuntime::new();
    let mut bounded = self::world(&mut bounded_host, renderer)?;
    for index in 0..129 {
        add(
            &mut bounded,
            0,
            Transform::default(),
            BoundingGeometry {
                geometry: geometry(2, 2.0 + f64::from(index) * 0.001),
                is_rendered: true,
                ..unit_geometry()
            },
        )?;
    }
    let capacity = super::world::render_frame(renderer, &mut bounded, WIDTH, HEIGHT)?;
    assert_eq!((capacity.draw_calls, capacity.failed_draw_calls), (129, 0));
    assert!(capacity.debug_resident_bytes > 0);
    let repeat = super::world::render_frame(renderer, &mut bounded, WIDTH, HEIGHT)?;
    assert_eq!((repeat.draw_calls, repeat.failed_draw_calls), (129, 0));
    assert_eq!(repeat.uploaded_bytes, 0);
    // Deleting one declaration releases its private mesh while another remains usable.
    let mut recovering_host = ipp_core::HostRuntime::new();
    let mut recovering = self::world(&mut recovering_host, renderer)?;
    let outline_mesh = ipp_core::services::asset_management::builtin::debug_mesh(
        &ipp_core::systems::geometry::GeometryPrimitiveVisual {
            outline: true,
            ..Default::default()
        },
    )?;
    let outline_bytes = outline_mesh.vertex_bytes() + std::mem::size_of_val(outline_mesh.indices());
    add(
        &mut recovering,
        0,
        Transform {
            x: -1.2,
            ..Transform::default()
        },
        BoundingGeometry {
            is_rendered: true,
            outline: true,
            has_color_override: true,
            r: 0.0,
            g: 0.0,
            b: 1.0,
            ..unit_geometry()
        },
    )?;
    let first = recovering.debug_render_items()[0].entity;
    add(
        &mut recovering,
        0,
        Transform {
            x: 1.2,
            ..Transform::default()
        },
        BoundingGeometry {
            outline: true,
            stroke: 0.05,
            is_rendered: true,
            has_color_override: true,
            r: 1.0,
            g: 0.0,
            b: 0.0,
            ..unit_geometry()
        },
    )?;
    let second = recovering.debug_render_items()[1].entity;
    let declaration = recovering.inspect(second).unwrap();
    let waiting = super::world::render_frame(renderer, &mut recovering, WIDTH, HEIGHT)?;
    assert_eq!((waiting.draw_calls, waiting.failed_draw_calls), (2, 0));
    let before = capture()?;
    save(output, "debug-shared-residency", &before)?;
    assert!(count(&before, [0, 0, 255, 255]) > 100);
    assert!(count(&before, [255, 0, 0, 255]) > 100);
    assert_eq!(
        renderer
            .render(&mut recovering, WIDTH, HEIGHT)?
            .uploaded_bytes,
        0
    );
    apply(
        &mut recovering,
        vec![Command::Delete {
            entity: EntityRef::Handle(first),
        }],
    )?;
    let resumed = super::world::render_frame(renderer, &mut recovering, WIDTH, HEIGHT)?;
    assert_eq!((resumed.draw_calls, resumed.failed_draw_calls), (1, 0));
    assert_eq!(resumed.uploaded_bytes, 0);
    assert_eq!(resumed.debug_resident_bytes as usize, outline_bytes);
    assert_eq!(recovering.inspect(second).unwrap(), declaration);
    let after = capture()?;
    save(output, "debug-release-preserved", &after)?;
    assert!(count(&after, [255, 0, 0, 255]) > 100);
    assert_eq!(count(&after, [0, 0, 255, 255]), 0);
    Ok(())
}

fn geometry(shape: u32, height: f64) -> Vec<u8> {
    let shape = match shape {
        1 => GeometryShape::Sphere {
            center: [0.0; 3],
            radius: 1.0,
        },
        2 => GeometryShape::Pill {
            start: [0.0, -height / 2.0 + 0.5, 0.0],
            end: [0.0, height / 2.0 - 0.5, 0.0],
            radius: 0.5,
        },
        3 => GeometryShape::Box {
            min: [-1.0, -1.0, 0.0],
            max: [1.0, 1.0, 0.0],
        },
        _ => GeometryShape::Box {
            min: [-1.0; 3],
            max: [1.0; 3],
        },
    };
    GeometryDefinition::from(shape).encode().unwrap()
}

fn unit_geometry() -> BoundingGeometry {
    BoundingGeometry {
        geometry: geometry(0, 2.0),
        ..BoundingGeometry::default()
    }
}

fn world<'a, D: RenderDevice>(
    host: &'a mut ipp_core::HostRuntime,
    renderer: &RenderService<D>,
) -> Result<WorldContext<'a>> {
    renderer.install(host)?;
    let mut world = super::world::fixture_world(host)?;
    let camera = world.active_camera().unwrap();
    apply(
        &mut world,
        vec![Command::InsertComponentValue {
            entity: EntityRef::Handle(camera),
            value: ComponentValue::Transform(Transform {
                z: 6.0,
                ..Transform::default()
            }),
        }],
    )?;
    Ok(world)
}

fn add(
    world: &mut WorldContext<'_>,
    alias: u32,
    transform: Transform,
    debug: BoundingGeometry,
) -> Result<()> {
    apply(
        world,
        vec![
            Command::Create {
                alias,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(alias),
                value: ComponentValue::Transform(transform),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(alias),
                value: ComponentValue::BoundingGeometry(debug),
            },
        ],
    )
}

fn update(world: &mut WorldContext<'_>, patch: RenderStatePatch) -> Result<()> {
    world.enqueue_render_state_update(patch)?;
    world.step(0.0)?;
    let state = world.render_state();
    if let Some(visible) = patch.show_all_debug_geometries {
        assert_eq!(state.show_all_debug_geometries, visible);
    }
    if let Some(color) = patch.debug_geometry_color {
        assert_eq!(state.debug_geometry_color, color);
    }
    Ok(())
}

fn ready<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    world: &mut WorldContext<'_>,
) -> Result<()> {
    super::world::render_frame(renderer, world, WIDTH, HEIGHT)?;
    assert!(world.take_resource_requests().is_empty());
    assert!(world.resource_snapshots().is_empty());
    Ok(())
}

fn count(pixels: &[u8], color: [u8; 4]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| matches(pixel, &color))
        .count()
}
