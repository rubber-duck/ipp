//! Real Host/World/RenderService GLES scenario for retained GUI paint.
//!
//! The scenario exercises evaluated clip intersection, empty-clip submission
//! suppression, painter order, externally delivered font/drawing/bitmap
//! assets, independent named-part paint, and RenderService device replacement
//! through the production Surface path.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod smoke;

#[cfg(target_os = "linux")]
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(target_os = "linux")]
const SESSION: u64 = 1;

#[cfg(target_os = "linux")]
const BACKGROUND: [u8; 4] = [10, 14, 20, 255];

#[cfg(target_os = "linux")]
fn pixel(frame: &[u8], x: u32, y: u32) -> [u8; 4] {
    let offset = ((y * smoke::world::WIDTH + x) * 4) as usize;
    frame[offset..offset + 4].try_into().unwrap()
}

#[cfg(target_os = "linux")]
fn check(frame: &[u8], x: u32, y: u32, expected: [u8; 4], label: &str) -> Result<()> {
    let actual = pixel(frame, x, y);
    if actual
        .iter()
        .zip(expected)
        .all(|(actual, expected)| actual.abs_diff(expected) <= 8)
    {
        Ok(())
    } else {
        Err(format!("{label}: pixel ({x},{y}) = {actual:?}, expected {expected:?}").into())
    }
}

#[cfg(target_os = "linux")]
fn step(host: &mut ipp_core::HostRuntime, world: ipp_core::WorldId) -> Result<()> {
    host.world_mut(world).unwrap().step(0.0)?;
    Ok(())
}

#[cfg(target_os = "linux")]
#[allow(clippy::too_many_arguments)]
fn insert_node(
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
    entity: ipp_core::EntityId,
    root_incarnation: u64,
    id: u32,
    parent: Option<u32>,
    index: u32,
    content: ipp_core::GuiNodeContent,
    style: ipp_core::GuiNodeStyle,
) -> Result<()> {
    host.world_mut(world).unwrap().enqueue_gui_command(
        SESSION,
        ipp_core::GuiCommand::InsertNode {
            entity,
            root_incarnation,
            id: ipp_core::GuiNodeId(id),
            parent: parent.map(ipp_core::GuiNodeId),
            index,
            content,
            style,
        },
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn colored(width: f32, height: f32, color: [f32; 4]) -> ipp_core::GuiNodeStyle {
    ipp_core::GuiNodeStyle {
        width: Some(width),
        height: Some(height),
        background_color: Some(color),
        ..Default::default()
    }
}

#[cfg(target_os = "linux")]
fn render(
    renderer: &mut ipp_render_gl::RenderService<ipp_render_gl::GlesRenderDevice>,
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
) -> Result<ipp_render_gl::RenderStats> {
    smoke::world::render_host_frame(
        renderer,
        host,
        world,
        smoke::world::WIDTH,
        smoke::world::HEIGHT,
    )
    .map_err(Into::into)
}

#[cfg(target_os = "linux")]
fn gui_assets_are_prepared(
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
    panel: ipp_core::EntityId,
    sources: &[ipp_core::services::asset_management::AssetSource],
) -> bool {
    let world = host.world_mut(world).unwrap();
    let Some(keys) = sources
        .iter()
        .map(|source| world.asset_resources().find(source))
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let prepared: std::collections::BTreeSet<_> = world
        .surface_render_items()
        .iter()
        .find(|item| item.entity == panel)
        .into_iter()
        .flat_map(|item| &item.primitives)
        .filter_map(|primitive| match primitive {
            ipp_core::SurfaceRenderPrimitive::Glyphs {
                font,
                ..
            } => Some(font.key),
            ipp_core::SurfaceRenderPrimitive::Drawing {
                drawing,
                ..
            } => Some(drawing.key),
            ipp_core::SurfaceRenderPrimitive::Bitmap {
                bitmap,
                ..
            } => Some(bitmap.key),
            ipp_core::SurfaceRenderPrimitive::Box {
                ..
            } => None,
        })
        .collect();
    keys.into_iter().all(|key| {
        prepared.contains(&key)
            && world.asset_resources().get(key).is_some_and(|resource| {
                resource.data().is_some() && resource.graphics_ready() == Some(true)
            })
    })
}

#[cfg(target_os = "linux")]
fn settle_assets(
    renderer: &mut ipp_render_gl::RenderService<ipp_render_gl::GlesRenderDevice>,
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
    panel: ipp_core::EntityId,
    sources: &[ipp_core::services::asset_management::AssetSource],
    payloads: &std::collections::BTreeMap<String, Vec<u8>>,
) -> Result<ipp_render_gl::RenderStats> {
    let progress_limit = payloads
        .values()
        .map(|bytes| {
            bytes
                .len()
                .div_ceil(ipp_core::services::asset_management::STREAM_CAPACITY)
        })
        .sum::<usize>()
        + payloads.len() * 8;
    let mut last = ipp_render_gl::RenderStats::default();
    for _ in 0..progress_limit {
        host.progress_assets();
        for request in host.take_resource_requests() {
            let bytes = payloads
                .get(&request.source)
                .ok_or_else(|| format!("unexpected GUI asset request {}", request.source))?;
            host.complete_resource(request.id, Ok(bytes.clone()))?;
        }
        step(host, world)?;
        last = render(renderer, host, world)?;
        if gui_assets_are_prepared(host, world, panel, sources) && last.failed_draw_calls == 0 {
            return Ok(last);
        }
    }
    Err(format!("GUI assets did not settle through RenderService: {last:?}").into())
}

#[cfg(target_os = "linux")]
fn main() -> Result<()> {
    use ipp_core::components::{Camera, Hierarchy, Transform};
    use ipp_core::services::asset_management::{
        AssetSource, drawing::DRAWING_TYPE, font::FONT_TYPE,
    };
    use ipp_core::{
        Batch, Command, ComponentValue, EntityRef, GuiCommand, GuiContainerKind, GuiNodeContent,
        GuiNodeHandle, GuiNodeId, GuiNodePatch, GuiNodeStyle, GuiRoot, Surface, TEXTURE_TYPE,
    };
    use std::path::PathBuf;

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: egl_gui_layout <EGL-GLES-library-directory> <surface-assets-directory> <font-assets-directory> <evidence-directory>".into());
    }
    let library_dir = PathBuf::from(&args[0]);
    let assets = PathBuf::from(&args[1]);
    let fonts = PathBuf::from(&args[2]);
    let evidence = PathBuf::from(&args[3]);
    std::fs::create_dir_all(&evidence)?;

    let context =
        smoke::egl::Context::new(&library_dir, smoke::world::WIDTH, smoke::world::HEIGHT)?;
    let mut renderer = ipp_render_gl::RenderService::new(context.device()?)?;
    let mut host = ipp_core::HostRuntime::new();
    renderer.install(&mut host)?;
    host.data_sources_mut().register_stream("fixture://")?;
    let world = host.create_world(Default::default())?;

    let mut surface = Surface::default();
    surface.width = 4.0;
    surface.height = 2.0;
    let (camera, panel, rear_parent) = {
        let mut world_context = host.world_mut(world).unwrap();
        world_context.enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::Transform(Transform {
                        z: 5.0,
                        ..Default::default()
                    }),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(1),
                    value: ComponentValue::Camera(Camera {
                        projection: 1,
                        ortho_height: 3.0,
                        ..Default::default()
                    }),
                },
                Command::Create {
                    alias: 2,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(2),
                    value: ComponentValue::Transform(Transform::default()),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(2),
                    value: ComponentValue::Surface(surface),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(2),
                    value: ComponentValue::GuiRoot(GuiRoot::default()),
                },
                Command::Create {
                    alias: 3,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(3),
                    value: ComponentValue::Transform(Transform {
                        qy: 1.0,
                        qw: 0.0,
                        ..Default::default()
                    }),
                },
            ],
        })?;
        let report = world_context.step(0.0)?;
        let created = report.outcomes[0]
            .result
            .as_ref()
            .map_err(|error| format!("GUI setup batch failed: {error:?}"))?;
        (created[0].1, created[1].1, created[2].1)
    };
    host.world_mut(world)
        .unwrap()
        .enqueue_camera_activate(camera)?;
    step(&mut host, world)?;
    assert_eq!(host.world_mut(world).unwrap().active_camera(), Some(camera));

    let root_incarnation = host
        .world_mut(world)
        .unwrap()
        .inspect_gui(panel, None, 1, 1)?
        .root_incarnation;
    let container = GuiNodeContent::Container;
    insert_node(
        &mut host,
        world,
        panel,
        root_incarnation,
        1,
        None,
        0,
        container(GuiContainerKind::Stack),
        GuiNodeStyle {
            width: Some(4.0),
            height: Some(2.0),
            ..Default::default()
        },
    )?;
    insert_node(
        &mut host,
        world,
        panel,
        root_incarnation,
        2,
        Some(1),
        0,
        container(GuiContainerKind::ScrollView),
        GuiNodeStyle {
            width: Some(0.0),
            height: Some(0.0),
            ..Default::default()
        },
    )?;
    insert_node(
        &mut host,
        world,
        panel,
        root_incarnation,
        3,
        Some(2),
        0,
        container(GuiContainerKind::Column),
        GuiNodeStyle::default(),
    )?;
    insert_node(
        &mut host,
        world,
        panel,
        root_incarnation,
        4,
        Some(3),
        0,
        container(GuiContainerKind::SizedBox),
        colored(4.0, 1.0, [1.0, 0.0, 1.0, 1.0]),
    )?;
    step(&mut host, world)?;

    let empty_stats = render(&mut renderer, &mut host, world)?;
    assert_eq!(empty_stats.draw_calls, 0);
    let empty_frame = context.capture()?;
    assert!(
        empty_frame
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| *pixel == BACKGROUND)
    );
    std::fs::write(evidence.join("gui-layout-empty.rgba"), &empty_frame)?;

    host.world_mut(world).unwrap().enqueue_gui_command(
        SESSION,
        GuiCommand::UpdateNode {
            handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(2), 1),
            patch: GuiNodePatch {
                width: Some(Some(4.0)),
                height: Some(Some(0.5)),
                ..Default::default()
            },
        },
    )?;
    step(&mut host, world)?;
    let clipped_stats = render(&mut renderer, &mut host, world)?;
    assert_eq!(clipped_stats.draw_calls, 1);
    let clipped_frame = context.capture()?;
    check(
        &clipped_frame,
        160,
        60,
        [255, 0, 255, 255],
        "intersected scroll clip interior",
    )?;
    check(
        &clipped_frame,
        160,
        100,
        BACKGROUND,
        "content below the scroll intersection",
    )?;
    std::fs::write(evidence.join("gui-layout-clipped.rgba"), &clipped_frame)?;

    let drawing = AssetSource {
        kind: DRAWING_TYPE,
        uri: "fixture:///panel.ippd".into(),
        variant: 0,
    };
    let font = AssetSource {
        kind: FONT_TYPE,
        uri: "fixture:///shure-tech-mono.ippf".into(),
        variant: 0,
    };
    let bitmap = AssetSource {
        kind: TEXTURE_TYPE,
        uri: "fixture:///badge.ippt".into(),
        variant: 0,
    };
    insert_node(
        &mut host,
        world,
        panel,
        root_incarnation,
        5,
        Some(1),
        1,
        container(GuiContainerKind::SizedBox),
        colored(1.5, 1.0, [1.0, 0.0, 0.0, 1.0]),
    )?;
    insert_node(
        &mut host,
        world,
        panel,
        root_incarnation,
        6,
        Some(1),
        2,
        container(GuiContainerKind::SizedBox),
        colored(1.5, 1.0, [0.0, 1.0, 0.0, 1.0]),
    )?;
    insert_node(
        &mut host,
        world,
        panel,
        root_incarnation,
        7,
        Some(1),
        3,
        GuiNodeContent::Drawing,
        GuiNodeStyle {
            width: Some(1.0),
            height: Some(1.0),
            margin: Some([0.75, 0.0, 0.0, 2.5]),
            color: [0.2, 0.6, 1.0, 1.0],
            asset: Some(drawing.clone()),
            ..Default::default()
        },
    )?;
    insert_node(
        &mut host,
        world,
        panel,
        root_incarnation,
        8,
        Some(1),
        4,
        GuiNodeContent::Text("A".into()),
        GuiNodeStyle {
            font_size: 0.35,
            margin: Some([1.2, 0.0, 0.0, 2.8]),
            color: [1.0, 1.0, 1.0, 1.0],
            asset: Some(font.clone()),
            ..Default::default()
        },
    )?;
    insert_node(
        &mut host,
        world,
        panel,
        root_incarnation,
        9,
        Some(1),
        5,
        GuiNodeContent::Image {
            size: [0.4, 0.4],
        },
        GuiNodeStyle {
            margin: Some([1.3, 0.0, 0.0, 3.3]),
            asset: Some(bitmap.clone()),
            ..Default::default()
        },
    )?;
    step(&mut host, world)?;

    let named_properties = [
        (
            GuiRoot::part_property_name(GuiNodeId(8), "label", "color").unwrap(),
            ipp_core::DynamicValue::Vec4([1.0, 0.75, 0.1, 1.0]),
        ),
        (
            GuiRoot::part_property_name(GuiNodeId(7), "icon", "asset").unwrap(),
            ipp_core::DynamicValue::Asset(drawing.clone()),
        ),
        (
            GuiRoot::part_property_name(GuiNodeId(9), "icon", "color").unwrap(),
            ipp_core::DynamicValue::Vec4([0.75, 1.0, 0.75, 1.0]),
        ),
    ];
    host.world_mut(world).unwrap().enqueue(Batch {
        id: 2,
        operations: named_properties
            .into_iter()
            .map(|(name, value)| Command::SetDynamicProperty {
                entity: EntityRef::Handle(panel),
                component: ComponentValue::GUI_ROOT,
                name,
                value,
            })
            .collect(),
    })?;
    step(&mut host, world)?;

    let sources = [font.clone(), drawing.clone(), bitmap.clone()];
    let payloads = std::collections::BTreeMap::from([
        (
            font.uri.clone(),
            std::fs::read(fonts.join("shure-tech-mono.ippf"))?,
        ),
        (
            drawing.uri.clone(),
            std::fs::read(assets.join("panel.ippd"))?,
        ),
        (
            bitmap.uri.clone(),
            std::fs::read(assets.join("badge.ippt"))?,
        ),
    ]);
    let layered_stats = settle_assets(&mut renderer, &mut host, world, panel, &sources, &payloads)?;
    let drawing_key = host
        .world_mut(world)
        .unwrap()
        .asset_resources()
        .find(&drawing)
        .unwrap();
    let layered = context.capture()?;
    check(
        &layered,
        40,
        60,
        [0, 255, 0, 255],
        "later green sibling wins painter overlap",
    )?;
    check(
        &layered,
        312,
        100,
        BACKGROUND,
        "scroll clip remains effective beside layered siblings",
    )?;
    check(
        &layered,
        200,
        100,
        [124, 203, 255, 255],
        "drawing honors its stack margin beside the clipped content",
    )?;
    std::fs::write(evidence.join("gui-layout-layered.rgba"), &layered)?;

    host.world_mut(world).unwrap().enqueue_gui_command(
        SESSION,
        GuiCommand::MoveNode {
            handle: GuiNodeHandle::new(SESSION, panel, root_incarnation, GuiNodeId(6), 1),
            parent: Some(GuiNodeId(1)),
            index: 1,
        },
    )?;
    step(&mut host, world)?;
    let reordered_stats = render(&mut renderer, &mut host, world)?;
    assert_eq!(reordered_stats.draw_calls, layered_stats.draw_calls);
    let reordered = context.capture()?;
    check(
        &reordered,
        40,
        60,
        [255, 0, 0, 255],
        "reordered red sibling wins painter overlap",
    )?;
    std::fs::write(evidence.join("gui-layout-reordered.rgba"), &reordered)?;

    // Perspective retained paint: turning the panel 30 degrees under a 60 degree
    // perspective camera draws the same retained batches at independently projected
    // positions. Restoring the view reproduces the front frame exactly.
    let turn = 30.0_f32.to_radians();
    let set_view = |host: &mut ipp_core::HostRuntime, perspective: bool| -> Result<()> {
        use ipp_core::{FieldValue, FieldWrite};
        use std::mem::offset_of;

        let (half_sin, half_cos) = if perspective {
            (turn * 0.5).sin_cos()
        } else {
            (0.0, 1.0)
        };
        let field = |entity, component, offset: usize, value| Command::SetField {
            entity: EntityRef::Handle(entity),
            component,
            field: FieldWrite {
                offset: offset as u32,
                value,
            },
        };
        let mut world_context = host.world_mut(world).unwrap();
        world_context.enqueue(Batch {
            id: world_context.tick() + 1,
            operations: vec![
                field(
                    camera,
                    ComponentValue::CAMERA,
                    offset_of!(Camera, projection),
                    FieldValue::U32(u32::from(!perspective)),
                ),
                field(
                    camera,
                    ComponentValue::CAMERA,
                    offset_of!(Camera, fov_y),
                    FieldValue::F32(60.0_f32.to_radians()),
                ),
                field(
                    panel,
                    ComponentValue::TRANSFORM,
                    offset_of!(Transform, qy),
                    FieldValue::F32(half_sin),
                ),
                field(
                    panel,
                    ComponentValue::TRANSFORM,
                    offset_of!(Transform, qw),
                    FieldValue::F32(half_cos),
                ),
            ],
        })?;
        let report = world_context.step(0.0)?;
        report.outcomes[0]
            .result
            .as_ref()
            .map_err(|error| format!("view change failed: {error:?}"))?;
        Ok(())
    };

    // Content (x, y) lies at local (x - 2, 1 - y) on the panel, turned about +Y and
    // viewed from z = 5 with the default near and far planes.
    let project = |[x, y]: [f32; 2]| -> [f32; 2] {
        let (sin, cos) = turn.sin_cos();
        let local = [x - 2.0, 1.0 - y];
        let eye = [local[0] * cos, local[1], -local[0] * sin - 5.0];
        let focal = 1.0 / 30.0_f32.to_radians().tan();
        let aspect = smoke::world::WIDTH as f32 / smoke::world::HEIGHT as f32;
        let ndc = [focal / aspect * eye[0] / -eye[2], focal * eye[1] / -eye[2]];
        [
            (ndc[0] + 1.0) * 0.5 * smoke::world::WIDTH as f32,
            (1.0 - ndc[1]) * 0.5 * smoke::world::HEIGHT as f32,
        ]
    };
    let front = |[x, y]: [f32; 2]| [80.0 * x, 40.0 + 80.0 * y];

    // Label pixels inside the projected bounds of the text's content rectangle, and
    // that rectangle's projected area.
    let label_box = [[2.8, 1.2], [3.25, 1.2], [3.25, 1.65], [2.8, 1.65]];
    let label_region = |frame: &[u8], map: &dyn Fn([f32; 2]) -> [f32; 2]| {
        let corners = label_box.map(map);
        let (xs, ys) = (corners.map(|[x, _]| x), corners.map(|[_, y]| y));
        let [left, right] = [
            xs.iter().copied().fold(f32::MAX, f32::min),
            xs.iter().copied().fold(f32::MIN, f32::max),
        ];
        let [top, bottom] = [
            ys.iter().copied().fold(f32::MAX, f32::min),
            ys.iter().copied().fold(f32::MIN, f32::max),
        ];
        let pixels = (top as u32..=bottom as u32)
            .flat_map(|y| (left as u32..=right as u32).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let [red, green, blue, _] = pixel(frame, x, y);
                red > 200 && green > 150 && blue < 140
            })
            .count() as f32;
        let area = (0..4)
            .map(|index| {
                let [a, b] = [corners[index], corners[(index + 1) % 4]];
                a[0] * b[1] - b[0] * a[1]
            })
            .sum::<f32>()
            .abs()
            * 0.5;
        (pixels, area)
    };
    let (front_label, front_area) = label_region(&reordered, &front);

    set_view(&mut host, true)?;
    let perspective_stats = render(&mut renderer, &mut host, world)?;
    let perspective = context.capture()?;
    std::fs::write(evidence.join("gui-layout-perspective.rgba"), &perspective)?;
    assert_eq!(perspective_stats.gui_batches, reordered_stats.gui_batches);
    assert!(perspective_stats.glyph_pages > 0);
    for (point, expected, label) in [
        ([0.75, 0.75], [255, 0, 0, 255], "perspective red sibling"),
        (
            [3.6, 0.25],
            [255, 0, 255, 255],
            "perspective scroll content",
        ),
        ([1.9, 1.6], BACKGROUND, "perspective transparent stack"),
    ] {
        let [x, y] = project(point);
        check(&perspective, x as u32, y as u32, expected, label)?;
    }

    // Thin strokes lose some thresholded pixels at the smaller projected size, so
    // the label keeps at least half of its front pixel density.
    let (perspective_label, perspective_area) = label_region(&perspective, &project);
    let minimum_label = 0.5 * front_label * perspective_area / front_area;
    if front_label < 20.0 || perspective_label < minimum_label {
        return Err(format!(
            "perspective label kept {perspective_label} of {front_label} front pixels"
        )
        .into());
    }

    set_view(&mut host, false)?;
    render(&mut renderer, &mut host, world)?;
    if context.capture()? != reordered {
        return Err("restoring the front view changed its completed GUI frame".into());
    }

    let show_rear = |host: &mut ipp_core::HostRuntime| -> Result<()> {
        let mut world_context = host.world_mut(world).unwrap();
        world_context.enqueue(Batch {
            id: world_context.tick() + 1,
            operations: vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(panel),
                value: ComponentValue::Hierarchy(Hierarchy {
                    parent: rear_parent,
                    ..Default::default()
                }),
            }],
        })?;
        world_context.step(0.0)?;
        Ok(())
    };
    show_rear(&mut host)?;
    let rear_stats = render(&mut renderer, &mut host, world)?;
    assert_eq!(rear_stats.draw_calls, reordered_stats.draw_calls);
    let rear = context.capture()?;
    check(
        &rear,
        279,
        60,
        [255, 0, 0, 255],
        "rear GUI keeps mirrored painter order",
    )?;
    let mut mirror_mismatches = 0usize;
    for y in 0..smoke::world::HEIGHT {
        for x in 0..smoke::world::WIDTH {
            let expected = pixel(&reordered, x, y);
            let actual = pixel(&rear, smoke::world::WIDTH - x - 1, y);
            if expected
                .iter()
                .zip(actual)
                .any(|(expected, actual)| expected.abs_diff(actual) > 8)
            {
                mirror_mismatches += 1;
            }
        }
    }
    if mirror_mismatches > 300 {
        return Err(format!(
            "rear GUI frame is not the mirrored live front: {mirror_mismatches} mismatches"
        )
        .into());
    }
    std::fs::write(evidence.join("gui-layout-rear.rgba"), &rear)?;
    {
        let mut world_context = host.world_mut(world).unwrap();
        world_context.enqueue(Batch {
            id: world_context.tick() + 1,
            operations: vec![Command::RemoveComponent {
                entity: EntityRef::Handle(panel),
                component: ComponentValue::HIERARCHY,
            }],
        })?;
        world_context.step(0.0)?;
    }
    render(&mut renderer, &mut host, world)?;
    if context.capture()? != reordered {
        return Err("restoring the GUI front changed its completed frame".into());
    }

    renderer.replace_device(&mut host, context.device()?)?;
    let recovered_stats =
        settle_assets(&mut renderer, &mut host, world, panel, &sources, &payloads)?;
    let recovered = context.capture()?;
    std::fs::write(evidence.join("gui-layout-recovered.rgba"), &recovered)?;
    check(
        &recovered,
        40,
        60,
        [255, 0, 0, 255],
        "painter order survives device replacement",
    )?;
    if recovered != reordered {
        return Err("device replacement changed the completed GUI frame".into());
    }

    let world_context = host.world_mut(world).unwrap();
    world_context.bounding_geometry(panel)?;
    assert_eq!(world_context.active_camera(), Some(camera));
    std::fs::write(
        evidence.join("gui-layout-production.txt"),
        format!(
            "empty_draw_calls={}\nclipped_draw_calls={}\nlayered_draw_calls={}\nreordered_draw_calls={}\nperspective_draw_calls={}\nperspective_label_pixels={perspective_label}/{front_label}\nrear_draw_calls={}\nrear_mirror_mismatches={}\nrecovered_draw_calls={}\ndrawing_key={}:{}\nfont={}\nbitmap={}\n",
            empty_stats.draw_calls,
            clipped_stats.draw_calls,
            layered_stats.draw_calls,
            reordered_stats.draw_calls,
            perspective_stats.draw_calls,
            rear_stats.draw_calls,
            mirror_mismatches,
            recovered_stats.draw_calls,
            drawing_key.slot,
            drawing_key.generation,
            font.uri,
            bitmap.uri,
        ),
    )?;

    println!(
        "PASS: production GUI clips, painter order, font/drawing/bitmap assets and device recovery"
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The EGL GUI layout runner supports Linux; no graphics test was run.");
    std::process::exit(1);
}
