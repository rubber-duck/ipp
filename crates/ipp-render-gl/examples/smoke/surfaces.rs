//! Real Host/World/asset-provider Surface frame scenario.
//!
//! The fixture camera World places a Surface anchor whose attachment presents
//! the Canvas output of a child World. Raw Canvas drawing, text and bitmap
//! entities in that child receive fonts, drawings and bitmaps through the
//! Host's streamed asset provider.

use std::{collections::BTreeMap, path::Path};

use ipp_core::{
    Batch, CanvasState, Command, ComponentValue, EntityId, EntityPlacementRef, EntityRef,
    HostRuntime, OutputRef, WorldAttachment, WorldCreateOptions, WorldId,
    components::{CanvasBitmap, CanvasDrawing, CanvasStyle, CanvasText, Surface, Transform},
    services::asset_management::{AssetSource, AssetTypeId, STREAM_CAPACITY},
};
use ipp_render_gl::{RenderDevice, RenderService};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Physical Surface extent in metres; one Canvas logical unit per metre.
const EXTENT: [f32; 2] = [2.2, 1.4];

/// Queue one ordered batch and apply it in a Host frame.
fn apply(
    host: &mut HostRuntime,
    world: WorldId,
    operations: Vec<Command>,
) -> Result<Vec<(u32, EntityId)>> {
    host.world_mut(world)
        .ok_or_else(|| format!("unknown World {world:?}"))?
        .enqueue(Batch {
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
        .map_err(|error| format!("Surface fixture batch failed: {error:?}"))?)
}

/// Create one entity with `values`, optionally as the last child of `parent`.
fn create(
    host: &mut HostRuntime,
    world: WorldId,
    parent: Option<EntityId>,
    values: Vec<ComponentValue>,
) -> Result<EntityId> {
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

/// Place one top-level raw Canvas leaf with its authored placement, scale and tint.
fn leaf(
    host: &mut HostRuntime,
    canvas: OutputRef,
    content: ComponentValue,
    style: CanvasStyle,
) -> Result<EntityId> {
    create(
        host,
        canvas.world().id(),
        None,
        vec![content, ComponentValue::CanvasStyle(style)],
    )
}

fn drawing(source: &AssetSource) -> ComponentValue {
    ComponentValue::CanvasDrawing(CanvasDrawing {
        source: source.uri.clone(),
        variant: source.variant,
    })
}

pub(crate) fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    assets: &Path,
    fonts: &Path,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    mut rebuild: impl FnMut() -> Result<D>,
    output: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    host.data_sources_mut().register_stream("fixture://")?;
    let id = super::world::fixture_world(&mut host)?.id();

    let mut options = WorldCreateOptions::new(super::selection::panel());
    options.canvas = Some(CanvasState {
        extent: EXTENT,
        units_per_metre: 1.0,
    });
    let content = host.create_world_with_options(Default::default(), options)?;
    let canvas = OutputRef::canvas(host.world_ref(content).unwrap());

    let source = |kind, name: &str| AssetSource {
        kind: AssetTypeId(kind),
        uri: format!("fixture:///{name}").into(),
        variant: 0,
    };
    let font = source(17, "shure-tech-mono.ippf");
    let panel = source(18, "panel.ippd");
    let icon = source(18, "icon.ippd");
    let badge = source(2, "badge.ippt");
    let leaves = vec![
        leaf(
            &mut host,
            canvas,
            drawing(&panel),
            CanvasStyle {
                x: 1.1,
                y: 0.7,
                scale_x: 1.9,
                scale_y: 1.1,
                red: 0.35,
                green: 0.45,
                blue: 0.7,
                alpha: 0.75,
                ..Default::default()
            },
        )?,
        leaf(
            &mut host,
            canvas,
            drawing(&icon),
            CanvasStyle {
                x: 0.28,
                y: 0.6,
                scale_x: 0.025,
                scale_y: 0.025,
                ..Default::default()
            },
        )?,
        leaf(
            &mut host,
            canvas,
            ComponentValue::CanvasText(CanvasText {
                text: "AOg0".into(),
                source: font.uri.clone(),
                variant: font.variant,
                font_size: 0.28,
            }),
            CanvasStyle {
                x: 0.65,
                y: 0.65,
                red: 1.0,
                green: 0.8,
                blue: 0.15,
                ..Default::default()
            },
        )?,
        leaf(
            &mut host,
            canvas,
            ComponentValue::CanvasBitmap(CanvasBitmap {
                source: badge.uri.clone(),
                variant: badge.variant,
                width: 0.42,
                height: 0.42,
            }),
            CanvasStyle {
                x: 1.64,
                y: 0.41,
                opacity: 0.7,
                ..Default::default()
            },
        )?,
    ];

    let surface = Surface {
        width: EXTENT[0],
        height: EXTENT[1],
        ..Default::default()
    };
    let entity = create(
        &mut host,
        id,
        None,
        vec![
            ComponentValue::Transform(Transform::default()),
            ComponentValue::Surface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(canvas)),
        ],
    )?;

    let payloads = BTreeMap::from([
        (
            font.uri.clone(),
            std::fs::read(fonts.join("shure-tech-mono.ippf"))?,
        ),
        (panel.uri.clone(), std::fs::read(assets.join("panel.ippd"))?),
        (icon.uri.clone(), std::fs::read(assets.join("icon.ippd"))?),
        (badge.uri.clone(), std::fs::read(assets.join("badge.ippt"))?),
    ]);
    let progress_limit = payloads
        .values()
        .map(|bytes| bytes.len().div_ceil(STREAM_CAPACITY))
        .sum::<usize>()
        + payloads.len() * 4;
    let mut ready_stats = None;
    for _ in 0..progress_limit {
        host.progress_assets();
        for request in host.take_resource_requests() {
            let bytes = payloads
                .get(&request.source)
                .ok_or_else(|| format!("unexpected Surface request {}", request.source))?;
            host.complete_resource(request.id, Ok(bytes.clone()))?;
        }

        let stats = super::world::render_host_frame(
            renderer,
            &mut host,
            id,
            super::world::WIDTH,
            super::world::HEIGHT,
        )?;
        if stats.triangles > stats.draw_calls * 2 {
            ready_stats = Some(stats);
            break;
        }
    }
    let stats = ready_stats.ok_or_else(|| {
        let resource = host
            .asset_resources()
            .find(&font)
            .and_then(|key| host.asset_resources().get(key));
        format!(
            "Surface glyph GPU resources did not become ready: {:?}",
            resource.map(|resource| (resource.status(), resource.representation()))
        )
    })?;
    if stats.draw_calls < 5 {
        return Err(format!("Surface scene submitted too few draws: {stats:?}").into());
    }
    if stats.triangles <= stats.draw_calls * 2 {
        return Err(format!("Surface instances were absent from triangle stats: {stats:?}").into());
    }
    let pixels = capture()?;
    let changed = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| **pixel != [10, 14, 20, 255])
        .count();
    if changed < 2_000 {
        return Err(format!("Surface scene coverage too small: {changed}").into());
    }
    super::world::save(output, "surface-runtime", &pixels)?;
    std::fs::write(output.join("surface-runtime.rgba"), &pixels)?;
    std::fs::write(
        output.join("surface-runtime.txt"),
        format!(
            "draw_calls={}\ntriangles={}\nchanged_pixels={changed}\n",
            stats.draw_calls, stats.triangles
        ),
    )?;
    rear_view(
        renderer,
        &mut host,
        id,
        entity,
        &mut capture,
        &pixels,
        output,
    )?;

    renderer.replace_device(&mut host, rebuild()?)?;
    for _ in 0..16 {
        host.progress_assets();
        for request in host.take_resource_requests() {
            let bytes = payloads
                .get(&request.source)
                .ok_or_else(|| format!("unexpected recovery request {}", request.source))?;
            host.complete_resource(request.id, Ok(bytes.clone()))?;
        }

        if super::world::render_host_frame(
            renderer,
            &mut host,
            id,
            super::world::WIDTH,
            super::world::HEIGHT,
        )
        .is_ok_and(|stats| stats.draw_calls >= 5)
        {
            break;
        }
    }
    let recovered = capture()?;
    let recovered_changed = recovered
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| **pixel != [10, 14, 20, 255])
        .count();
    if recovered_changed.abs_diff(changed) > 32 {
        return Err(format!(
            "Surface context recovery changed coverage: {changed} -> {recovered_changed}"
        )
        .into());
    }
    super::world::save(output, "surface-runtime-recovered", &recovered)?;
    orientation(
        renderer,
        &mut host,
        id,
        entity,
        canvas,
        &leaves,
        &panel,
        &mut capture,
        output,
    )?;
    renderer.prepare(&mut host, None)?;
    Ok(())
}

/// The complete live Surface remains visible from behind with comparable
/// glyph/drawing/bitmap coverage and a visibly reflected asymmetric image.
fn rear_view<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut HostRuntime,
    id: WorldId,
    entity: EntityId,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    front: &[u8],
    output: &Path,
) -> Result<()> {
    let parent = create(
        host,
        id,
        None,
        vec![ComponentValue::Transform(Transform {
            qy: 1.0,
            qw: 0.0,
            ..Default::default()
        })],
    )?;
    apply(
        host,
        id,
        vec![Command::PlaceEntity {
            entity: EntityRef::Handle(entity),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        }],
    )?;
    super::world::render_host_frame(
        renderer,
        host,
        id,
        super::world::WIDTH,
        super::world::HEIGHT,
    )?;
    let rear = capture()?;
    let summarize = |pixels: &[u8]| {
        let mut content = 0usize;
        let mut glyph = 0usize;
        for pixel in pixels.as_chunks::<4>().0 {
            if *pixel != [10, 14, 20, 255] {
                content += 1;
            }
            if pixel[0] > 180 && pixel[1] > 130 && pixel[2] < 140 {
                glyph += 1;
            }
        }
        (content, glyph)
    };
    let (front_content, front_glyph) = summarize(front);
    let (rear_content, rear_glyph) = summarize(&rear);
    let different = front
        .as_chunks::<4>()
        .0
        .iter()
        .zip(rear.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count();
    if rear_content < 2_000
        || rear_glyph < 80
        || rear_content.abs_diff(front_content) > front_content / 20
        || rear_glyph.abs_diff(front_glyph) > front_glyph / 5
        || different < 1_000
    {
        return Err(format!(
            "rear Surface coverage mismatch: front=({front_content},{front_glyph}) rear=({rear_content},{rear_glyph}) different={different}"
        )
        .into());
    }
    super::world::save(output, "surface-runtime-rear", &rear)?;
    std::fs::write(
        output.join("surface-runtime-rear.txt"),
        format!(
            "front_content={front_content}\nrear_content={rear_content}\nfront_glyph={front_glyph}\nrear_glyph={rear_glyph}\ndifferent_pixels={different}\n"
        ),
    )?;
    apply(
        host,
        id,
        vec![
            Command::PlaceEntity {
                entity: EntityRef::Handle(entity),
                placement: EntityPlacementRef::default(),
            },
            Command::Delete {
                entity: EntityRef::Handle(parent),
            },
        ],
    )?;
    Ok(())
}

/// Top-left/Y-down content must appear top-left on screen and follow the entity transform.
#[allow(clippy::too_many_arguments)]
fn orientation<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut HostRuntime,
    id: WorldId,
    entity: EntityId,
    canvas: OutputRef,
    leaves: &[EntityId],
    panel: &AssetSource,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    const RED: [f32; 3] = [1.0, 0.0, 0.0];
    const GREEN: [f32; 3] = [0.0, 1.0, 0.0];

    // The attached Canvas now holds only two asymmetric markers. They are added
    // before the old leaves go, so the shared drawing stays referenced.
    for (position, color) in [([0.3, 0.3], RED), ([1.9, 1.1], GREEN)] {
        leaf(
            host,
            canvas,
            drawing(panel),
            CanvasStyle {
                x: position[0],
                y: position[1],
                scale_x: 0.3,
                scale_y: 0.3,
                red: color[0],
                green: color[1],
                blue: color[2],
                ..Default::default()
            },
        )?;
    }
    apply(
        host,
        canvas.world().id(),
        leaves
            .iter()
            .map(|leaf| Command::Delete {
                entity: EntityRef::Handle(*leaf),
            })
            .collect(),
    )?;

    let rotation = |half_turn: bool| {
        let (qz, qw) = if half_turn {
            (1.0, 0.0)
        } else {
            (0.0, 1.0)
        };
        [
            (std::mem::offset_of!(Transform, qz), qz),
            (std::mem::offset_of!(Transform, qw), qw),
        ]
        .map(|(offset, value)| Command::SetField {
            entity: EntityRef::Handle(entity),
            component: ComponentValue::TRANSFORM,
            field: ipp_core::FieldWrite {
                offset: offset as u32,
                value: ipp_core::FieldValue::F32(value),
            },
        })
    };
    let mut centroids = Vec::new();
    for half_turn in [false, true] {
        apply(host, id, rotation(half_turn).to_vec())?;
        super::world::render_host_frame(
            renderer,
            host,
            id,
            super::world::WIDTH,
            super::world::HEIGHT,
        )?;
        let pixels = capture()?;
        let label = if half_turn {
            "surface-orientation-rotated"
        } else {
            "surface-orientation"
        };
        super::world::save(output, label, &pixels)?;
        let centroid = |select: fn(&[u8; 4]) -> bool| {
            let (mut count, mut x, mut y) = (0usize, 0usize, 0usize);
            for (index, pixel) in pixels.as_chunks::<4>().0.iter().enumerate() {
                if select(pixel) {
                    count += 1;
                    x += index % super::world::WIDTH as usize;
                    y += index / super::world::WIDTH as usize;
                }
            }
            (count > 40)
                .then(|| [x as f64 / count as f64, y as f64 / count as f64])
                .ok_or_else(|| format!("{label}: marker coverage {count}"))
        };
        let red = centroid(|pixel| pixel[0] > 180 && pixel[1] < 90 && pixel[2] < 90)?;
        let green = centroid(|pixel| pixel[1] > 180 && pixel[0] < 90 && pixel[2] < 90)?;
        centroids.push((red, green));
    }
    // Image rows grow downward. Unrotated, content (0.3, 0.3) is left of and above
    // content (1.9, 1.1); a half turn about the entity's Z axis swaps both relations.
    let [(red, green), (turned_red, turned_green)] = centroids[..] else {
        unreachable!("two captures")
    };
    if !(red[0] + 20.0 < green[0] && red[1] + 10.0 < green[1]) {
        return Err(format!("Surface content orientation: red {red:?}, green {green:?}").into());
    }
    if !(turned_red[0] > turned_green[0] + 20.0 && turned_red[1] > turned_green[1] + 10.0) {
        return Err(format!(
            "rotated Surface placement: red {turned_red:?}, green {turned_green:?}"
        )
        .into());
    }
    std::fs::write(
        output.join("surface-orientation.txt"),
        format!(
            "red={red:?}\ngreen={green:?}\nrotated_red={turned_red:?}\nrotated_green={turned_green:?}\n"
        ),
    )?;
    Ok(())
}
