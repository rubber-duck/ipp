//! Real raw-Canvas publication captures using the shared EGL scenario driver.

use super::publications::{apply, apply_batch, assert_color, camera, create, mesh, save};
use ipp_core::components::{CanvasBox, CanvasStyle, FlatSurface, Transform};
use ipp_core::{
    Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef, HostRuntime, OutputRef,
    WorldAttachment, WorldId, WorldViewport,
};
use ipp_render_gl::{RenderDevice, RenderService};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Top-level content of the canvas World.
pub(super) fn place(
    host: &mut HostRuntime,
    parent: OutputRef,
    values: Vec<ComponentValue>,
) -> Result<EntityId> {
    create(host, parent.world().id(), values)
}

/// The World canvas with a 128 x 128 extent at `density` units per metre,
/// applied at the World's next mutation boundary.
pub(super) fn canvas(host: &mut HostRuntime, world: WorldId, density: f32) -> Result<OutputRef> {
    host.world_mut(world)
        .ok_or("missing canvas World")?
        .enqueue_canvas_state_update(ipp_core::CanvasStateUpdate {
            extent: Some([128.0, 128.0]),
            units_per_metre: Some(density),
        })?;
    Ok(OutputRef::canvas(host.world_ref(world).unwrap()))
}

fn shape(
    host: &mut HostRuntime,
    parent: OutputRef,
    position: [f32; 2],
    size: [f32; 2],
    color: [f32; 3],
) -> Result<EntityId> {
    place(
        host,
        parent,
        vec![
            ComponentValue::CanvasBox(CanvasBox {
                width: size[0],
                height: size[1],
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: position[0],
                y: position[1],
                red: color[0],
                green: color[1],
                blue: color[2],
                ..Default::default()
            }),
        ],
    )
}

pub(super) fn attach(
    host: &mut HostRuntime,
    parent: OutputRef,
    child: OutputRef,
    position: [f32; 2],
    extent: [f32; 2],
    clipped: bool,
) -> Result<EntityId> {
    let surface = FlatSurface {
        width: extent[0],
        height: extent[1],
        ..Default::default()
    };
    place(
        host,
        parent,
        vec![
            ComponentValue::FlatSurface(surface),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: position[0],
                y: position[1],
                clipped,
                clip_min_x: 8.0,
                clip_min_y: 4.0,
                clip_max_x: 56.0,
                clip_max_y: 44.0,
                ..Default::default()
            }),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child)),
        ],
    )
}

fn frame<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut HostRuntime,
    root: OutputRef,
    viewport: WorldViewport,
) -> Result<()> {
    frame_at(renderer, host, root, viewport, 0.0)
}

pub(super) fn frame_at<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut HostRuntime,
    root: OutputRef,
    viewport: WorldViewport,
    time: f64,
) -> Result<()> {
    for _ in 0..4 {
        let report = host.frame(time)?;
        assert!(
            report.publication_errors.is_empty(),
            "{:?}",
            report.publication_errors
        );
        assert!(
            report.worlds.values().all(std::result::Result::is_ok),
            "{:?}",
            report.worlds
        );
        renderer.prepare(
            host,
            host.root_output(root.world().id())
                .map(|(output, _, publication)| (output, publication)),
        )?;
        host.progress_assets();
    }

    let publication = host.root_output(root.world().id()).unwrap().2;
    assert_eq!(
        renderer
            .draw(host, root, publication, viewport, time)?
            .failed_draw_calls,
        0
    );
    Ok(())
}

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let parent = host.create_world(Default::default(), &super::selection::panel())?;
    let child = host.create_world(Default::default(), &super::selection::panel())?;
    let scene_world = host.create_world(Default::default(), &super::selection::scene())?;
    let root = canvas(&mut host, parent, 100.0)?;
    let nested = canvas(&mut host, child, 200.0)?;
    let scene = camera(&mut host, scene_world, 1.0)?;
    mesh(
        &mut host,
        scene_world,
        Transform {
            sx: 4.0,
            sy: 4.0,
            ..Default::default()
        },
        [0.5, 0.25, 0.0],
    )?;
    shape(
        &mut host,
        root,
        [0.0, 0.0],
        [128.0, 128.0],
        [0.05, 0.1, 0.15],
    )?;
    shape(&mut host, root, [8.0, 12.0], [32.0, 20.0], [0.0, 0.5, 0.0])?;
    shape(
        &mut host,
        nested,
        [0.0, 0.0],
        [128.0, 96.0],
        [0.25, 0.5, 0.75],
    )?;
    shape(
        &mut host,
        nested,
        [0.0, 0.0],
        [64.0, 48.0],
        [0.75, 0.75, 0.0],
    )?;
    attach(&mut host, nested, scene, [64.0, 48.0], [0.32, 0.24], false)?;
    attach(&mut host, root, nested, [48.0, 8.0], [0.64, 0.48], true)?;
    shape(
        &mut host,
        root,
        [68.0, 28.0],
        [12.0, 12.0],
        [0.75, 0.0, 0.0],
    )?;
    let viewport = WorldViewport {
        width: 256,
        height: 256,
        device_pixel_ratio: 2.0,
    };
    host.set_root_output(root, viewport)?;
    frame(renderer, &mut host, root, viewport)?;
    let pixels = capture()?;
    save(output, "canvas-root-nested-clipped", &pixels)?;
    for (position, color) in [
        ([40, 40], [0.0, 0.5, 0.0]),
        ([100, 32], [0.05, 0.1, 0.15]),
        ([120, 40], [0.75, 0.75, 0.0]),
        ([180, 40], [0.25, 0.5, 0.75]),
        ([180, 80], [0.5, 0.25, 0.0]),
        ([144, 64], [0.75, 0.0, 0.0]),
        ([216, 80], [0.05, 0.1, 0.15]),
        ([40, 216], [0.05, 0.1, 0.15]),
    ] {
        assert_color(&pixels, position[0], position[1], color);
    }
    frame(renderer, &mut host, root, viewport)?;
    assert_eq!(renderer.statistics().gui_rebuilds, 0);
    assert_eq!(renderer.statistics().uploaded_bytes, 0);
    assert_eq!(capture()?, pixels);

    let camera_world = host.create_world(Default::default(), &super::selection::scene())?;
    let camera_root = camera(&mut host, camera_world, 2.0)?;
    host.clear_root_output(parent);
    let surface = FlatSurface {
        width: 1.28,
        height: 1.28,
        ..Default::default()
    };
    create(
        &mut host,
        camera_world,
        vec![
            ComponentValue::FlatSurface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(root)),
        ],
    )?;
    let viewport = WorldViewport {
        device_pixel_ratio: 1.0,
        ..viewport
    };
    host.set_root_output(camera_root, viewport)?;
    frame(renderer, &mut host, camera_root, viewport)?;
    let pixels = capture()?;
    save(output, "camera-canvas-camera", &pixels)?;
    assert_color(&pixels, 72, 72, [0.0, 0.5, 0.0]);
    assert_color(&pixels, 161, 72, [0.25, 0.5, 0.75]);
    assert_color(&pixels, 161, 98, [0.5, 0.25, 0.0]);
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    opacity_cache(renderer, &mut capture, output)?;
    nested_cache_refresh(renderer, &mut capture, output)?;
    Ok(())
}

fn style(
    host: &mut HostRuntime,
    world: WorldId,
    entity: EntityId,
    value: CanvasStyle,
) -> Result<()> {
    apply(
        host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::CanvasStyle(value),
        )],
    )?;
    Ok(())
}

fn opacity_cache<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let parent = host.create_world(Default::default(), &super::selection::panel())?;
    let child = host.create_world(Default::default(), &super::selection::panel())?;
    let scene_world = host.create_world(Default::default(), &super::selection::scene())?;
    let root = canvas(&mut host, parent, 100.0)?;
    let nested = canvas(&mut host, child, 100.0)?;
    let scene = camera(&mut host, scene_world, 1.0)?;
    mesh(
        &mut host,
        scene_world,
        Transform {
            sx: 4.0,
            sy: 4.0,
            ..Default::default()
        },
        [0.0, 1.0, 0.0],
    )?;
    shape(&mut host, root, [0.0; 2], [256.0; 2], [0.0; 3])?;
    let red = shape(&mut host, nested, [0.0; 2], [90.0; 2], [1.0, 0.0, 0.0])?;
    style(
        &mut host,
        child,
        red,
        CanvasStyle {
            red: 1.0,
            green: 0.0,
            blue: 0.0,
            alpha: 0.5,
            ..Default::default()
        },
    )?;
    let blue = shape(&mut host, nested, [40.0; 2], [60.0; 2], [0.0, 0.0, 1.0])?;
    style(
        &mut host,
        child,
        blue,
        CanvasStyle {
            x: 40.0,
            y: 40.0,
            red: 0.0,
            green: 0.0,
            blue: 1.0,
            alpha: 0.5,
            ..Default::default()
        },
    )?;
    let camera_anchor = attach(&mut host, nested, scene, [0.0, 100.0], [0.64, 0.24], false)?;
    style(
        &mut host,
        child,
        camera_anchor,
        CanvasStyle {
            y: 100.0,
            opacity: 0.5,
            ..Default::default()
        },
    )?;
    let anchor = attach(&mut host, root, nested, [0.0; 2], [1.28; 2], false)?;
    let ancestor = place(
        &mut host,
        root,
        vec![ComponentValue::CanvasStyle(CanvasStyle::default())],
    )?;
    apply(
        &mut host,
        parent,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(anchor),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(ancestor)),
                before: None,
            },
        }],
    )?;
    let viewport = WorldViewport {
        width: 256,
        height: 256,
        device_pixel_ratio: 2.0,
    };
    host.set_root_output(root, viewport)?;
    for (index, (local, inherited)) in [(1.0, 1.0), (0.5, 1.0), (0.0, 1.0), (0.5, 0.5), (1.0, 0.0)]
        .into_iter()
        .enumerate()
    {
        style(
            &mut host,
            parent,
            anchor,
            CanvasStyle {
                opacity: local,
                ..Default::default()
            },
        )?;
        style(
            &mut host,
            parent,
            ancestor,
            CanvasStyle {
                opacity: inherited,
                ..Default::default()
            },
        )?;
        renderer.set_surface_cache_budget(0);
        let time = index as f64;
        frame_at(renderer, &mut host, root, viewport, time)?;
        let direct = capture()?;
        let alpha = local * inherited * 0.5;
        assert_color(&direct, 40, 40, [alpha, 0.0, 0.0]);
        assert_color(&direct, 120, 120, [alpha * (1.0 - alpha), 0.0, alpha]);
        assert_color(&direct, 40, 220, [0.0, local * inherited * 0.5, 0.0]);
        save(output, &format!("canvas-opacity-{index}-direct"), &direct)?;
        apply(
            &mut host,
            parent,
            vec![Command::insert_value(
                EntityRef::Handle(anchor),
                ComponentValue::SurfaceCache(ipp_core::components::SurfaceCache {
                    direct_distance: 0.0,
                    texels_per_metre: 200.0,
                    max_refresh_hz: 1.0,
                }),
            )],
        )?;
        renderer.set_surface_cache_budget(ipp_render_gl::SURFACE_CACHE_BUDGET_BYTES);
        frame_at(renderer, &mut host, root, viewport, time)?;
        let cached = capture()?;
        save(output, &format!("canvas-opacity-{index}-cached"), &cached)?;
        for position in [[40, 40], [120, 120], [40, 220]] {
            let offset = (position[1] * 256 + position[0]) * 4;
            assert!(
                direct[offset..offset + 4]
                    .iter()
                    .zip(&cached[offset..offset + 4])
                    .all(|(left, right)| left.abs_diff(*right) <= 2),
                "direct/cache opacity disagreement at {position:?}: {:?} vs {:?}",
                &direct[offset..offset + 4],
                &cached[offset..offset + 4]
            );
        }

        if local * inherited > 0.0 {
            assert_eq!(renderer.statistics().surface_cache_repaints, 1);
            frame_at(renderer, &mut host, root, viewport, time)?;
            assert_eq!(renderer.statistics().surface_cache_repaints, 0);
            assert_eq!(renderer.statistics().surface_cache_reuses, 1);
            assert_eq!(capture()?, cached);
        }
    }
    style(&mut host, parent, anchor, CanvasStyle::default())?;
    style(&mut host, parent, ancestor, CanvasStyle::default())?;
    frame_at(renderer, &mut host, root, viewport, 10.0)?;
    let parent_publication = host.root_output(parent).unwrap().2;
    let entries = host
        .output(parent_publication, root)
        .unwrap()
        .data::<ipp_core::systems::canvas::CanvasPublication>()
        .unwrap()
        .entries
        .clone();
    style(
        &mut host,
        child,
        blue,
        CanvasStyle {
            x: 40.0,
            y: 40.0,
            red: 0.0,
            green: 1.0,
            blue: 0.0,
            alpha: 0.5,
            ..Default::default()
        },
    )?;
    frame_at(renderer, &mut host, root, viewport, 10.25)?;
    assert_eq!(renderer.statistics().surface_cache_repaints, 0);
    assert_color(&capture()?, 120, 120, [0.25, 0.0, 0.5]);
    frame_at(renderer, &mut host, root, viewport, 11.0)?;
    assert_eq!(renderer.statistics().surface_cache_repaints, 1);
    let changed = capture()?;
    save(output, "canvas-cache-child-paint", &changed)?;
    assert_color(&changed, 120, 120, [0.25, 0.5, 0.0]);
    let current = host.root_output(parent).unwrap().2;
    assert!(std::sync::Arc::ptr_eq(
        &entries,
        &host
            .output(current, root)
            .unwrap()
            .data::<ipp_core::systems::canvas::CanvasPublication>()
            .unwrap()
            .entries
    ));
    let child_publication = host.latest_publication(child).unwrap();
    let token = host
        .publication(child_publication)
        .unwrap()
        .attachments
        .iter()
        .find(|edge| edge.anchor == camera_anchor)
        .unwrap()
        .token
        .clone();
    let outcome = apply_batch(
        &mut host,
        child,
        ipp_core::Batch {
            id: 30,
            operations: vec![
                Command::DetachWorldAttachmentIf {
                    expected: token.clone(),
                },
                Command::SetField {
                    entity: EntityRef::Handle(camera_anchor),
                    component: ComponentValue::CANVAS_STYLE,
                    field: ipp_core::FieldWrite {
                        offset: std::mem::offset_of!(CanvasStyle, opacity) as u32,
                        value: ipp_core::FieldValue::F32(1.1),
                    },
                },
            ],
        },
    )?;
    assert_eq!(
        outcome.result.unwrap_err().reason,
        ipp_core::ErrorReason::InvalidValue
    );
    assert_eq!(
        host.attachment_retirement(&token),
        Ok(ipp_core::WorldAttachmentRetirement::Retired)
    );
    frame_at(renderer, &mut host, root, viewport, 11.1)?;
    assert_eq!(renderer.statistics().surface_cache_repaints, 1);
    let retired = capture()?;
    save(output, "canvas-cache-retired-child", &retired)?;
    assert_color(&retired, 40, 220, [0.0; 3]);
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    Ok(())
}

fn nested_cache_refresh<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let mut outputs = Vec::new();
    for _ in 0..3 {
        let world = host.create_world(Default::default(), &super::selection::panel())?;
        outputs.push(canvas(&mut host, world, 100.0)?);
    }

    let root = outputs[0];
    shape(&mut host, root, [0.0; 2], [128.0; 2], [0.0; 3])?;
    for (index, pair) in outputs.windows(2).enumerate() {
        let anchor = attach(&mut host, pair[0], pair[1], [0.0; 2], [1.28; 2], false)?;
        apply(
            &mut host,
            pair[0].world().id(),
            vec![
                Command::insert_value(
                    EntityRef::Handle(anchor),
                    ComponentValue::CanvasStyle(CanvasStyle {
                        opacity: 0.5,
                        ..Default::default()
                    }),
                ),
                Command::insert_value(
                    EntityRef::Handle(anchor),
                    ComponentValue::SurfaceCache(ipp_core::components::SurfaceCache {
                        direct_distance: 0.0,
                        texels_per_metre: 200.0,
                        max_refresh_hz: if index == 0 {
                            4.0
                        } else {
                            1.0
                        },
                    }),
                ),
            ],
        )?;
    }

    let leaf = outputs[2];
    let shape = shape(&mut host, leaf, [0.0; 2], [128.0; 2], [1.0, 0.0, 0.0])?;
    let viewport = WorldViewport {
        width: 256,
        height: 256,
        device_pixel_ratio: 2.0,
    };
    host.set_root_output(root, viewport)?;
    frame_at(renderer, &mut host, root, viewport, 0.0)?;
    assert_color(&capture()?, 128, 128, [0.25, 0.0, 0.0]);
    style(
        &mut host,
        leaf.world().id(),
        shape,
        CanvasStyle {
            red: 0.0,
            green: 0.0,
            blue: 1.0,
            ..Default::default()
        },
    )?;
    frame_at(renderer, &mut host, root, viewport, 0.5)?;
    assert_color(&capture()?, 128, 128, [0.25, 0.0, 0.0]);
    frame_at(renderer, &mut host, root, viewport, 1.0)?;
    frame_at(renderer, &mut host, root, viewport, 1.1)?;
    let pixels = capture()?;
    save(output, "canvas-nested-cache-refresh", &pixels)?;
    assert_color(&pixels, 128, 128, [0.0, 0.0, 0.25]);
    frame_at(renderer, &mut host, root, viewport, 1.2)?;
    assert_eq!(renderer.statistics().surface_cache_repaints, 0);
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    Ok(())
}
