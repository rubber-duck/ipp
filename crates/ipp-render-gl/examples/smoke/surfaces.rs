//! Real Host/World/asset-provider Surface frame scenario.

use std::{collections::BTreeMap, path::Path};

use ipp_core::{
    Batch, Command, ComponentValue, EntityRef, Surface, SurfaceItemContent, SurfaceItemStyle,
    components::{Hierarchy, Transform},
    services::asset_management::{AssetSource, AssetTypeId, STREAM_CAPACITY},
};
use ipp_render_gl::{RenderDevice, RenderService};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub(crate) fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    assets: &Path,
    fonts: &Path,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    mut rebuild: impl FnMut() -> Result<D>,
    output: &Path,
) -> Result<()> {
    let mut host = ipp_core::HostRuntime::new();
    renderer.install(&mut host)?;
    host.data_sources_mut().register_stream("fixture://")?;
    let mut world = super::world::fixture_world(&mut host)?;

    let source = |kind, name: &str| AssetSource {
        kind: AssetTypeId(kind),
        uri: format!("fixture:///{name}"),
        variant: 0,
    };
    let font = source(17, "shure-tech-mono.ippf");
    let panel = source(18, "panel.ippd");
    let icon = source(18, "icon.ippd");
    let badge = source(2, "badge.ippt");
    let mut surface = Surface::default();
    surface.width = 2.2;
    surface.height = 1.4;
    surface.insert_item(
        0,
        SurfaceItemContent::Drawing,
        SurfaceItemStyle {
            position: [1.1, 0.7],
            scale: [1.9, 1.1],
            color: [0.35, 0.45, 0.7, 0.75],
            asset: Some(panel.clone()),
            ..Default::default()
        },
    )?;
    surface.insert_item(
        1,
        SurfaceItemContent::Drawing,
        SurfaceItemStyle {
            position: [0.28, 0.6],
            scale: [0.025, 0.025],
            asset: Some(icon.clone()),
            ..Default::default()
        },
    )?;
    surface.insert_item(
        2,
        SurfaceItemContent::Label("AOg0".into()),
        SurfaceItemStyle {
            position: [0.65, 0.65],
            color: [1.0, 0.8, 0.15, 1.0],
            font_size: 0.28,
            asset: Some(font.clone()),
            ..Default::default()
        },
    )?;
    surface.insert_item(
        3,
        SurfaceItemContent::Bitmap {
            size: [0.42, 0.42],
        },
        SurfaceItemStyle {
            position: [1.64, 0.41],
            opacity: 0.7,
            asset: Some(badge.clone()),
            ..Default::default()
        },
    )?;
    world.enqueue(Batch {
        id: 90,
        operations: vec![
            Command::Create {
                alias: 9,
                metadata: Default::default(),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(9),
                value: ComponentValue::Transform(Transform::default()),
            },
            Command::InsertComponentValue {
                entity: EntityRef::Alias(9),
                value: ComponentValue::Surface(surface),
            },
        ],
    })?;
    let entity = world.step(0.0)?.outcomes[0]
        .result
        .as_ref()
        .map_err(|error| format!("Surface fixture batch failed: {error:?}"))?[0]
        .1;
    let id = world.id();
    drop(world);

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
        host.world_mut(id).unwrap().step(0.0)?;
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
        let world = host.world_mut(id).unwrap();
        let resource = world
            .asset_resources()
            .find(&font)
            .and_then(|key| world.asset_resources().get(key));
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
        host.world_mut(id).unwrap().step(0.0)?;
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
        &panel,
        &mut capture,
        output,
    )
}

/// The complete live Surface remains visible from behind with comparable
/// glyph/drawing/bitmap coverage and a visibly reflected asymmetric image.
fn rear_view<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut ipp_core::HostRuntime,
    id: ipp_core::WorldId,
    entity: ipp_core::EntityId,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    front: &[u8],
    output: &Path,
) -> Result<()> {
    let parent = {
        let mut world = host.world_mut(id).unwrap();
        world.enqueue(Batch {
            id: world.tick() + 1,
            operations: vec![
                Command::Create {
                    alias: 77,
                    metadata: Default::default(),
                },
                Command::InsertComponentValue {
                    entity: EntityRef::Alias(77),
                    value: ComponentValue::Transform(Transform {
                        qy: 1.0,
                        qw: 0.0,
                        ..Default::default()
                    }),
                },
            ],
        })?;
        world.step(0.0)?.outcomes[0]
            .result
            .as_ref()
            .map_err(|error| format!("rear parent creation failed: {error:?}"))?[0]
            .1
    };
    {
        let mut world = host.world_mut(id).unwrap();
        super::world::apply(
            &mut world,
            vec![Command::InsertComponentValue {
                entity: EntityRef::Handle(entity),
                value: ComponentValue::Hierarchy(Hierarchy {
                    parent,
                    ..Default::default()
                }),
            }],
        )?;
    }
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
    {
        let mut world = host.world_mut(id).unwrap();
        super::world::apply(
            &mut world,
            vec![
                Command::RemoveComponent {
                    entity: EntityRef::Handle(entity),
                    component: ComponentValue::HIERARCHY,
                },
                Command::Delete {
                    entity: EntityRef::Handle(parent),
                },
            ],
        )?;
    }
    Ok(())
}

/// Top-left/Y-down content must appear top-left on screen and follow the entity transform.
fn orientation<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    host: &mut ipp_core::HostRuntime,
    id: ipp_core::WorldId,
    entity: ipp_core::EntityId,
    panel: &AssetSource,
    capture: &mut impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
) -> Result<()> {
    const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
    const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

    let mut surface = Surface::default();
    surface.width = 2.2;
    surface.height = 1.4;
    for (index, (position, color)) in [([0.3, 0.3], RED), ([1.9, 1.1], GREEN)]
        .into_iter()
        .enumerate()
    {
        surface.insert_item(
            index,
            SurfaceItemContent::Drawing,
            SurfaceItemStyle {
                position,
                scale: [0.3, 0.3],
                color,
                asset: Some(panel.clone()),
                ..Default::default()
            },
        )?;
    }
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
    for (batch, half_turn) in [(91, false), (92, true)] {
        let mut operations = rotation(half_turn).to_vec();
        if !half_turn {
            operations.splice(
                0..0,
                [
                    Command::RemoveComponent {
                        entity: EntityRef::Handle(entity),
                        component: ComponentValue::SURFACE,
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Handle(entity),
                        value: ComponentValue::Surface(surface.clone()),
                    },
                ],
            );
        }
        {
            let mut world = host.world_mut(id).unwrap();
            world.enqueue(Batch {
                id: batch,
                operations,
            })?;
            world.step(0.0)?;
        }
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
