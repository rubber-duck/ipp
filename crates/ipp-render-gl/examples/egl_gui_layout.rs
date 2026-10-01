//! Real Host/World/RenderService GLES scenario for retained GUI paint.
//!
//! A camera World places a Surface anchor whose attachment presents the Canvas
//! output of a child World. Ordinary layout, Canvas leaf and control entities in
//! that child exercise evaluated clip intersection, empty-clip submission
//! suppression, painter order and core reordering, externally delivered
//! font/drawing/bitmap assets, control part overrides above a shared theme, and
//! RenderService device replacement.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod smoke;

#[cfg(target_os = "linux")]
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

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

/// Queue one ordered batch and apply it in a Host frame.
#[cfg(target_os = "linux")]
fn apply(
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
    operations: Vec<ipp_core::Command>,
) -> Result<Vec<(u32, ipp_core::EntityId)>> {
    host.world_mut(world)
        .ok_or_else(|| format!("unknown World {world:?}"))?
        .enqueue(ipp_core::Batch {
            id: 1,
            operations,
        })?;

    let update = host
        .frame(0.0)?
        .worlds
        .remove(&world)
        .ok_or_else(|| format!("World {world:?} was not updated"))?
        .map_err(|reason| format!("World {world:?} update: {reason:?}"))?;
    let outcome = update
        .outcomes
        .into_iter()
        .next()
        .ok_or_else(|| format!("World {world:?} reported no batch outcome"))?;

    Ok(outcome
        .result
        .map_err(|error| format!("GUI batch failed: {error:?}"))?)
}

/// Create one entity with `values`, optionally as the last child of `parent`.
#[cfg(target_os = "linux")]
fn create(
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
    parent: Option<ipp_core::EntityId>,
    values: Vec<ipp_core::ComponentValue>,
) -> Result<ipp_core::EntityId> {
    use ipp_core::{Command, EntityPlacementRef, EntityRef};

    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(0), value)),
    );
    if let Some(parent) = parent {
        operations.push(Command::PlaceEntity {
            entity: EntityRef::Alias(0),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        });
    }

    Ok(apply(host, world, operations)?[0].1)
}

#[cfg(target_os = "linux")]
fn render(
    renderer: &mut ipp_render_gl::RenderService<ipp_render_gl::GlesRenderDevice>,
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
) -> Result<crate::smoke::frame_stats::FrameStats> {
    smoke::world::render_host_frame(
        renderer,
        host,
        world,
        smoke::world::WIDTH,
        smoke::world::HEIGHT,
    )
    .map_err(Into::into)
}

/// The latest completed publication of the attached Canvas output.
#[cfg(target_os = "linux")]
fn canvas_publication(
    host: &ipp_core::HostRuntime,
    output: ipp_core::OutputRef,
) -> Option<ipp_core::systems::canvas::CanvasPublication> {
    host.output(host.latest_publication(output.world().id())?, output)?
        .data::<ipp_core::systems::canvas::CanvasPublication>()
        .cloned()
}

#[cfg(target_os = "linux")]
fn gui_assets_are_prepared(
    host: &ipp_core::HostRuntime,
    canvas: ipp_core::OutputRef,
    sources: &[ipp_core::services::asset_management::AssetSource],
) -> bool {
    let Some(keys) = sources
        .iter()
        .map(|source| host.asset_resources().find(source))
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let Some(publication) = canvas_publication(host, canvas) else {
        return false;
    };

    let prepared: std::collections::BTreeSet<_> = publication.resources().collect();
    keys.into_iter().all(|key| {
        prepared.contains(&key)
            && host.asset_resources().get(key).is_some_and(|resource| {
                resource.data().is_some() && resource.graphics_ready() == Some(true)
            })
    })
}

#[cfg(target_os = "linux")]
fn settle_assets(
    renderer: &mut ipp_render_gl::RenderService<ipp_render_gl::GlesRenderDevice>,
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
    canvas: ipp_core::OutputRef,
    sources: &[ipp_core::services::asset_management::AssetSource],
    payloads: &std::collections::BTreeMap<std::sync::Arc<str>, Vec<u8>>,
) -> Result<crate::smoke::frame_stats::FrameStats> {
    let progress_limit = payloads
        .values()
        .map(|bytes| {
            bytes
                .len()
                .div_ceil(ipp_core::services::asset_management::STREAM_CAPACITY)
        })
        .sum::<usize>()
        + payloads.len() * 8;
    let mut last = crate::smoke::frame_stats::FrameStats::default();
    for _ in 0..progress_limit {
        host.progress_assets();
        for request in host.take_resource_requests() {
            let bytes = payloads
                .get(&request.source)
                .ok_or_else(|| format!("unexpected GUI asset request {}", request.source))?;
            host.complete_resource(request.id, Ok(bytes.clone()))?;
        }

        last = render(renderer, host, world)?;
        if gui_assets_are_prepared(host, canvas, sources) && last.failed_draw_calls == 0 {
            return Ok(last);
        }
    }
    Err(format!("GUI assets did not settle through RenderService: {last:?}").into())
}

/// The published paint target of `entity`'s first primitive in painter order.
#[cfg(target_os = "linux")]
fn paint_target(
    publication: &ipp_core::systems::canvas::CanvasPublication,
    entity: ipp_core::EntityId,
) -> Option<(usize, ipp_core::systems::canvas::CanvasTarget)> {
    use ipp_core::systems::canvas::CanvasPaintEntry;

    publication
        .entries
        .iter()
        .enumerate()
        .find_map(|(index, entry)| match entry.as_ref() {
            CanvasPaintEntry::Primitive {
                primitive,
                ..
            } if primitive.style().identity.target.entity == entity => {
                Some((index, primitive.style().identity.target))
            }
            _ => None,
        })
}

#[cfg(target_os = "linux")]
fn main() -> Result<()> {
    use ipp_core::components::rows::Rows;
    use ipp_core::components::{
        Camera, CanvasBox, CanvasDrawing, CanvasStyle, GuiButton, GuiFont, GuiLayout, GuiSkin,
        GuiTheme, Surface, Transform,
    };
    use ipp_core::services::asset_management::{
        AssetSource, drawing::DRAWING_TYPE, font::FONT_TYPE,
    };
    use ipp_core::systems::gui::GuiPrimitivePart;
    use ipp_core::systems::gui::presentation::GuiPaintPart;
    use ipp_core::systems::gui::{GuiPartId, GuiSkinState};
    use ipp_core::{
        CanvasState, Command, ComponentValue, EntityPlacementRef, EntityRef, FieldValue,
        FieldWrite, OutputKind, OutputRef, TEXTURE_TYPE, WorldAttachment, WorldCreateOptions,
    };
    use std::mem::offset_of;
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
    let world = host.create_world(Default::default(), &smoke::selection::scene())?;

    // One logical unit per Surface metre keeps content coordinates in panel metres.
    let mut options = WorldCreateOptions::new(smoke::selection::panel());
    options.canvas = Some(CanvasState {
        extent: [4.0, 2.0],
        units_per_metre: 1.0,
    });
    let gui = host.create_world_with_options(Default::default(), options)?;
    let canvas = OutputRef::canvas(host.world_ref(gui).unwrap());

    let surface = Surface {
        width: 4.0,
        height: 2.0,
    };
    let camera = create(
        &mut host,
        world,
        None,
        vec![
            ComponentValue::Transform(Transform {
                z: 5.0,
                ..Default::default()
            }),
            ComponentValue::Camera(Camera {
                projection: 1,
                ortho_height: 3.0,
                ..Default::default()
            }),
        ],
    )?;
    let panel = create(
        &mut host,
        world,
        None,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::Surface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(canvas)),
        ],
    )?;
    let rear_parent = create(
        &mut host,
        world,
        None,
        vec![ComponentValue::Transform(Transform {
            qy: 1.0,
            qw: 0.0,
            ..Default::default()
        })],
    )?;
    host.world_mut(world)
        .unwrap()
        .enqueue_camera_activate(camera)?;
    host.frame(0.0)?;
    assert_eq!(host.world_mut(world).unwrap().active_camera(), Some(camera));
    let selection = host.bind_output(host.world_ref(world).unwrap(), camera, OutputKind::Camera)?;
    host.set_root_output(
        selection,
        ipp_core::WorldViewport {
            width: smoke::world::WIDTH,
            height: smoke::world::HEIGHT,
            device_pixel_ratio: 1.0,
        },
    )?;

    // A clipped stack holds a viewport whose local CanvasStyle clip starts empty.
    // The viewport's content column is laid out independently of that clip, so
    // the visible region is the clip rectangle intersected with the stack clip.
    let stack = create(
        &mut host,
        gui,
        None,
        vec![ComponentValue::GuiLayout(GuiLayout {
            kind: 3,
            width: 4.0,
            height: 2.0,
            clip: true,
            ..Default::default()
        })],
    )?;
    let viewport = create(
        &mut host,
        gui,
        Some(stack),
        vec![
            ComponentValue::GuiLayout(GuiLayout {
                kind: 3,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                clipped: true,
                ..Default::default()
            }),
        ],
    )?;
    let column = create(
        &mut host,
        gui,
        Some(viewport),
        vec![ComponentValue::GuiLayout(GuiLayout {
            kind: 2,
            ..Default::default()
        })],
    )?;
    let shape = |host: &mut ipp_core::HostRuntime,
                 parent,
                 size: [f32; 2],
                 color: [f32; 3]|
     -> Result<ipp_core::EntityId> {
        create(
            host,
            gui,
            Some(parent),
            vec![
                ComponentValue::CanvasBox(CanvasBox::default()),
                ComponentValue::GuiLayout(GuiLayout {
                    kind: 6,
                    width: size[0],
                    height: size[1],
                    ..Default::default()
                }),
                ComponentValue::CanvasStyle(CanvasStyle {
                    red: color[0],
                    green: color[1],
                    blue: color[2],
                    ..Default::default()
                }),
            ],
        )
    };
    shape(&mut host, column, [4.0, 1.0], [1.0, 0.0, 1.0])?;

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

    let clip_edge = |offset: usize, value: f32| Command::SetField {
        entity: EntityRef::Handle(viewport),
        component: ComponentValue::CANVAS_STYLE,
        field: FieldWrite {
            offset: offset as u32,
            value: FieldValue::F32(value),
        },
    };
    apply(
        &mut host,
        gui,
        vec![
            clip_edge(offset_of!(CanvasStyle, clip_max_x), 4.0),
            clip_edge(offset_of!(CanvasStyle, clip_max_y), 0.5),
        ],
    )?;
    let clipped_stats = render(&mut renderer, &mut host, world)?;
    assert_eq!(clipped_stats.draw_calls, 1);
    let clipped_frame = context.capture()?;
    check(
        &clipped_frame,
        160,
        60,
        [255, 0, 255, 255],
        "intersected viewport clip interior",
    )?;
    check(
        &clipped_frame,
        160,
        100,
        BACKGROUND,
        "content below the viewport intersection",
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
    let red = shape(&mut host, stack, [1.5, 1.0], [1.0, 0.0, 0.0])?;
    let green = shape(&mut host, stack, [1.5, 1.0], [0.0, 1.0, 0.0])?;

    // The drawing leaf keeps its view-box coordinates: its unit square is centred
    // on its stack margin point, tinted by its CanvasStyle.
    create(
        &mut host,
        gui,
        Some(stack),
        vec![
            ComponentValue::CanvasDrawing(CanvasDrawing {
                source: drawing.uri.clone(),
                variant: drawing.variant,
            }),
            ComponentValue::GuiLayout(GuiLayout {
                width: 1.0,
                height: 1.0,
                margin_top: 0.75,
                margin_left: 2.5,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                red: 0.2,
                green: 0.6,
                blue: 1.0,
                ..Default::default()
            }),
        ],
    )?;

    // A shared theme paints every control's idle background grey and label blue.
    // Each control's own part overrides take precedence over it for every state
    // and variant: the label is amber on a transparent background and the bitmap
    // arrives as a tinted Background asset.
    let mut theme_parts = Rows::new();
    for (part, color) in [
        (GuiPrimitivePart::Background, [0.5, 0.5, 0.5, 1.0]),
        (GuiPrimitivePart::Label, [0.3, 0.5, 1.0, 1.0]),
    ] {
        theme_parts
            .push(GuiPaintPart {
                color: Some(color),
                ..GuiPaintPart::keyed(GuiPartId::state(part, GuiSkinState::Idle))?
            })
            .unwrap();
    }
    let theme = create(
        &mut host,
        gui,
        None,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts: theme_parts,
        })],
    )?;
    let skin = |overrides: Vec<GuiPaintPart>| -> ComponentValue {
        let mut parts = Rows::new();
        for part in overrides {
            parts.push(part).unwrap();
        }
        ComponentValue::GuiSkin(GuiSkin {
            theme,
            parts,
            ..Default::default()
        })
    };
    let placed = |width: f32, height: f32, top: f32, left: f32| {
        ComponentValue::GuiLayout(GuiLayout {
            width,
            height,
            margin_top: top,
            margin_left: left,
            ..Default::default()
        })
    };
    let background = GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Background))?;
    create(
        &mut host,
        gui,
        Some(stack),
        vec![
            ComponentValue::GuiButton(GuiButton {
                label: "A".into(),
            }),
            ComponentValue::GuiFont(GuiFont {
                source: font.uri.clone(),
                variant: 0,
                font_size: 0.35,
            }),
            placed(-1.0, -1.0, 1.2, 2.8),
            skin(vec![
                GuiPaintPart {
                    color: Some([0.0; 4]),
                    ..background.clone()
                },
                GuiPaintPart {
                    color: Some([1.0, 0.75, 0.1, 1.0]),
                    ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Label))?
                },
            ]),
        ],
    )?;
    create(
        &mut host,
        gui,
        Some(stack),
        vec![
            ComponentValue::GuiButton(GuiButton::default()),
            placed(0.4, 0.4, 1.3, 3.3),
            skin(vec![GuiPaintPart {
                color: Some([0.75, 1.0, 0.75, 1.0]),
                asset: Some(bitmap.clone()),
                ..background
            }]),
        ],
    )?;

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
    let layered_stats =
        settle_assets(&mut renderer, &mut host, world, canvas, &sources, &payloads)?;
    let drawing_key = host.asset_resources().find(&drawing).unwrap();
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
        "viewport clip remains effective beside layered siblings",
    )?;
    check(
        &layered,
        200,
        100,
        [124, 203, 255, 255],
        "drawing honors its stack margin beside the clipped content",
    )?;
    std::fs::write(evidence.join("gui-layout-layered.rgba"), &layered)?;

    // Core reordering moves the green sibling before the red one; both keep
    // their entity and component identities while exchanging painter order.
    let before = canvas_publication(&host, canvas).ok_or("no layered Canvas publication")?;
    let (red_order, red_target) = paint_target(&before, red).ok_or("red box is not painted")?;
    let (green_order, green_target) =
        paint_target(&before, green).ok_or("green box is not painted")?;
    assert!(red_order < green_order);
    apply(
        &mut host,
        gui,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(green),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(stack)),
                before: Some(EntityRef::Handle(red)),
            },
        }],
    )?;
    let reordered_stats = render(&mut renderer, &mut host, world)?;
    assert_eq!(reordered_stats.draw_calls, layered_stats.draw_calls);
    let after = canvas_publication(&host, canvas).ok_or("no reordered Canvas publication")?;
    let (red_order, red_reordered) = paint_target(&after, red).ok_or("red box vanished")?;
    let (green_order, green_reordered) = paint_target(&after, green).ok_or("green box vanished")?;
    assert!(green_order < red_order);
    assert_eq!((red_reordered, green_reordered), (red_target, green_target));
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
        apply(
            host,
            world,
            vec![
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
        )?;
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
            "perspective viewport content",
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

    let place_panel =
        |host: &mut ipp_core::HostRuntime, parent: Option<ipp_core::EntityId>| -> Result<()> {
            apply(
                host,
                world,
                vec![Command::PlaceEntity {
                    entity: EntityRef::Handle(panel),
                    placement: EntityPlacementRef {
                        parent: parent.map(EntityRef::Handle),
                        before: None,
                    },
                }],
            )?;
            Ok(())
        };
    place_panel(&mut host, Some(rear_parent))?;
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
    place_panel(&mut host, None)?;
    render(&mut renderer, &mut host, world)?;
    if context.capture()? != reordered {
        return Err("restoring the GUI front changed its completed frame".into());
    }

    renderer.replace_device(&mut host, context.device()?)?;
    let recovered_stats =
        settle_assets(&mut renderer, &mut host, world, canvas, &sources, &payloads)?;
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
