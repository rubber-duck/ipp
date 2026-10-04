//! Conservative publication culling and retained nested output work on real GLES.

use super::canvas_publications::{attach, canvas, place};
use super::publications::{apply, camera, create, mesh, save};
use ipp_core::components::{
    BoundingGeometry, CanvasBox, CanvasStyle, CanvasText, FlatSurface, Transform,
};
use ipp_core::services::asset_management::{AssetSource, font::FONT_TYPE};
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, HostRuntime, OutputRef, WorldAttachment,
    WorldId, WorldViewport,
};
use ipp_render_gl::{RenderDevice, RenderFrameSummary, RenderService};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn surface(
    host: &mut HostRuntime,
    parent: WorldId,
    output: OutputRef,
    transform: Transform,
) -> Result<EntityId> {
    create(
        host,
        parent,
        vec![
            ComponentValue::FlatSurface(FlatSurface::default()),
            ComponentValue::Transform(transform),
            ComponentValue::WorldAttachment(WorldAttachment::surface(output)),
        ],
    )
}

fn shape(host: &mut HostRuntime, output: OutputRef, color: [f32; 3]) -> Result<()> {
    place(
        host,
        output,
        vec![
            ComponentValue::CanvasBox(CanvasBox {
                width: 1.0,
                height: 1.0,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                red: color[0],
                green: color[1],
                blue: color[2],
                ..Default::default()
            }),
        ],
    )?;
    Ok(())
}

fn draw<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut HostRuntime,
    output: OutputRef,
    viewport: WorldViewport,
) -> Result<RenderFrameSummary> {
    let report = host.frame(0.0)?;
    assert!(
        report.publication_errors.is_empty(),
        "{:?}",
        report.publication_errors
    );
    assert!(report.worlds.values().all(std::result::Result::is_ok));
    let publication = host.root_output(output.world().id()).unwrap().2;
    renderer.prepare(host, Some((output, publication)))?;
    host.progress_assets();
    let summary = renderer.draw(host, output, publication, viewport, 0.0)?;
    assert_eq!(summary.failed_draw_calls, 0);
    assert!(!summary.invalid_camera);
    Ok(summary)
}

fn placement(
    host: &mut HostRuntime,
    world: WorldId,
    anchor: EntityId,
    transform: Transform,
) -> Result<()> {
    host.world_mut(world).unwrap().enqueue(Batch {
        id: 89,
        operations: vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::Transform(transform),
        )],
    })?;
    Ok(())
}

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    artifacts: &Path,
    fonts: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let root_world = host.create_world(Default::default(), &super::selection::scene())?;
    let spatial = host.create_world(Default::default(), &super::selection::scene())?;
    let canvas_world = host.create_world(Default::default(), &super::selection::panel())?;
    let camera_world = host.create_world(Default::default(), &super::selection::scene())?;
    let nested_world = host.create_world(Default::default(), &super::selection::panel())?;
    let other_camera_world = host.create_world(Default::default(), &super::selection::scene())?;
    let root = camera(&mut host, root_world, 4.0)?;
    let content = canvas(&mut host, canvas_world, 1.0)?;
    let nested_camera = camera(&mut host, camera_world, 1.0)?;
    let nested_canvas = canvas(&mut host, nested_world, 1.0)?;
    let other_camera = camera(&mut host, other_camera_world, 1.0)?;
    let affine = Transform {
        x: -0.25,
        y: -0.125,
        sx: 1.5,
        sy: 0.75,
        qz: (std::f32::consts::PI / 12.0).sin(),
        qw: (std::f32::consts::PI / 12.0).cos(),
        ..Default::default()
    };
    let spatial_ref = host.world_ref(spatial).unwrap();
    let anchor = create(
        &mut host,
        root_world,
        vec![
            ComponentValue::Transform(affine),
            ComponentValue::WorldAttachment(WorldAttachment::spatial(spatial_ref)),
        ],
    )?;
    let canvas_anchor = surface(&mut host, spatial, content, Transform::default())?;
    surface(
        &mut host,
        spatial,
        other_camera,
        Transform {
            x: 1.1,
            ..Default::default()
        },
    )?;
    shape(&mut host, content, [1.0; 3])?;
    shape(&mut host, nested_canvas, [0.0, 0.75, 0.0])?;
    mesh(
        &mut host,
        camera_world,
        Transform {
            sx: 4.0,
            sy: 4.0,
            ..Default::default()
        },
        [0.75, 0.0, 0.0],
    )?;
    mesh(
        &mut host,
        other_camera_world,
        Transform {
            sx: 4.0,
            sy: 4.0,
            ..Default::default()
        },
        [0.0, 0.0, 0.75],
    )?;
    attach(
        &mut host,
        content,
        nested_camera,
        [0.5, 0.5],
        [0.5; 2],
        false,
    )?;
    surface(
        &mut host,
        camera_world,
        nested_canvas,
        Transform {
            x: -0.2,
            y: 0.2,
            z: 1.0,
            sx: 0.4,
            sy: 0.4,
            ..Default::default()
        },
    )?;

    let font = AssetSource {
        kind: FONT_TYPE,
        uri: format!("producer://{}/17/1", canvas_world.0).into(),
        variant: 0,
    };
    let bytes = std::fs::read(fonts.join("shure-tech-mono.ippf"))?;
    let progress_limit = bytes
        .len()
        .div_ceil(ipp_core::services::asset_management::STREAM_CAPACITY)
        + 32;
    host.asset_resources_mut()
        .register_client_source(canvas_world, font.clone(), bytes)?;
    let font_key = host.asset_resources().find(&font).unwrap();
    place(
        &mut host,
        content,
        vec![
            ComponentValue::CanvasText(CanvasText {
                text: "ABg".into(),
                source: font.uri,
                font_size: 0.3,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 0.05,
                y: 0.3,
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                ..Default::default()
            }),
        ],
    )?;
    let viewport = WorldViewport {
        width: 256,
        height: 256,
        device_pixel_ratio: 1.0,
    };
    host.set_root_output(root, viewport)?;
    for _ in 0..progress_limit {
        draw(renderer, &mut host, root, viewport)?;
        if host
            .asset_resources()
            .get(font_key)
            .and_then(|resource| resource.data())
            .is_some_and(|data| data.graphics_ready() != Some(false))
        {
            break;
        }
    }
    assert!(
        host.asset_resources()
            .get(font_key)
            .and_then(|resource| resource.data())
            .is_some_and(|data| data.graphics_ready() != Some(false))
    );
    for _ in 0..4 {
        draw(renderer, &mut host, root, viewport)?;
    }
    let visible_summary = draw(renderer, &mut host, root, viewport)?;
    assert!(visible_summary.draw_calls >= 5);
    let warm = *renderer.statistics();
    assert_eq!(
        (warm.uploaded_bytes, warm.gui_rebuilds, warm.glyph_populates),
        (0, 0, 0)
    );
    assert!(warm.glyph_pages > 0 && warm.gui_resident_bytes > 0);
    let visible = capture()?;
    save(artifacts, "surface-visibility-warm", &visible)?;
    let count = |color: [u8; 3]| {
        visible
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| {
                pixel[..3]
                    .iter()
                    .zip(color)
                    .all(|(actual, expected)| actual.abs_diff(expected) <= 3)
            })
            .count()
    };
    assert!(count([255; 3]) > 300, "outer Canvas missing");
    assert!(count([225, 0, 0]) > 80, "nested Camera missing");
    assert!(count([0, 225, 0]) > 10, "Canvas inside Camera missing");
    assert!(count([0, 0, 225]) > 300, "sibling Camera Surface missing");

    let mut evidence = format!("warm: {visible_summary:?} {warm:?}\n");
    for (index, (x, z)) in [(0.0, 10.0), (100.0, 100.0), (-100.0, -100.0)]
        .into_iter()
        .enumerate()
    {
        placement(
            &mut host,
            root_world,
            anchor,
            Transform {
                x,
                z,
                ..affine
            },
        )?;
        let culled = draw(renderer, &mut host, root, viewport)?;
        let work = renderer.statistics();
        assert_eq!(culled.draw_calls, 0);
        assert_eq!(
            (
                work.uploaded_bytes,
                work.gui_rebuilds,
                work.glyph_populates,
                work.glyph_page_retirements
            ),
            (0, 0, 0, 0)
        );
        assert_eq!(
            (work.glyph_pages, work.gui_resident_bytes),
            (warm.glyph_pages, warm.gui_resident_bytes)
        );
        let pixels = capture()?;
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel[..3] == [10, 14, 20])
        );
        save(
            artifacts,
            &format!("surface-visibility-culled-{index}"),
            &pixels,
        )?;
        evidence.push_str(&format!("culled-{index}: {culled:?} {work:?}\n"));
    }
    placement(&mut host, root_world, anchor, affine)?;
    let resumed = draw(renderer, &mut host, root, viewport)?;
    let work = renderer.statistics();
    assert_eq!(resumed.draw_calls, visible_summary.draw_calls);
    assert_eq!(
        (
            work.uploaded_bytes,
            work.gui_rebuilds,
            work.gui_allocations,
            work.glyph_populates,
            work.glyph_page_retirements
        ),
        (0, 0, 0, 0, 0)
    );
    let resumed_pixels = capture()?;
    assert_eq!(resumed_pixels, visible);
    save(artifacts, "surface-visibility-resumed", &resumed_pixels)?;
    evidence.push_str(&format!("resumed: {resumed:?} {work:?}\n"));

    apply(
        &mut host,
        spatial,
        vec![Command::insert_value(
            EntityRef::Handle(canvas_anchor),
            ComponentValue::BoundingGeometry(BoundingGeometry {
                source: "missing:///bounds.ippg".into(),
                ..Default::default()
            }),
        )],
    )?;
    placement(
        &mut host,
        root_world,
        anchor,
        Transform {
            x: 100.0,
            ..affine
        },
    )?;
    let unknown = draw(renderer, &mut host, root, viewport)?;
    assert!(unknown.draw_calls > 0, "unproven bounds must stay eligible");
    let publication = host
        .publication(host.latest_publication(spatial).unwrap())
        .unwrap();
    let geometry = publication
        .chunk(ipp_core::systems::geometry::GeometrySystem::ID)
        .unwrap()
        .data::<ipp_core::systems::geometry::GeometryPublication>()
        .unwrap();
    assert!(
        geometry
            .entities
            .iter()
            .find(|geometry| geometry.entity == canvas_anchor)
            .unwrap()
            .culling
            .is_none()
    );
    let outside = capture()?;
    assert!(
        outside
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[..3] == [10, 14, 20])
    );
    save(artifacts, "surface-visibility-unknown-eligible", &outside)?;
    evidence.push_str(&format!(
        "unknown: {unknown:?} {:?}\n",
        renderer.statistics()
    ));
    apply(
        &mut host,
        spatial,
        vec![Command::insert_value(
            EntityRef::Handle(canvas_anchor),
            ComponentValue::BoundingGeometry(Default::default()),
        )],
    )?;
    assert_eq!(draw(renderer, &mut host, root, viewport)?.draw_calls, 0);
    std::fs::write(artifacts.join("surface-visibility-work.txt"), evidence)?;
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    Ok(())
}
