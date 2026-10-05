//! Converted example assets through exact publication leases and the real GL providers.

use super::canvas_publications::{attach, canvas, frame_at, place};
use super::publications::{apply, assert_color, save};
use ipp_core::components::{CanvasBitmap, CanvasDrawing, CanvasStyle, CanvasText, SurfaceCache};
use ipp_core::services::asset_management::{AssetSource, AssetTypeId};
use ipp_core::{Command, ComponentValue, EntityRef, HostRuntime, WorldViewport};
use ipp_render_gl::{RenderDevice, RenderService};
use std::path::Path;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub fn run<D: RenderDevice>(
    renderer: &mut RenderService<D>,
    mut capture: impl FnMut() -> Result<Vec<u8>>,
    output: &Path,
    assets: &Path,
    fonts: &Path,
) -> Result<()> {
    let mut host = HostRuntime::new();
    renderer.install(&mut host)?;
    let parent = host.create_world(Default::default(), &super::selection::panel())?;
    let child = host.create_world(Default::default(), &super::selection::panel())?;
    let root = canvas(&mut host, parent, 100.0)?;
    let nested = canvas(&mut host, child, 100.0)?;
    let source = |kind, slot| AssetSource {
        kind: AssetTypeId(kind),
        uri: format!("producer://{}/{kind}/{slot}", child.0).into(),
        variant: 0,
    };

    let font = source(17, 1);
    let drawing = source(18, 2);
    let bitmap = source(2, 3);
    let badge = std::fs::read(assets.join("badge.ippt"))?;
    let payloads = [
        (
            font.clone(),
            std::fs::read(fonts.join("shure-tech-mono.ippf"))?,
        ),
        (drawing.clone(), std::fs::read(assets.join("panel.ippd"))?),
        (bitmap.clone(), badge.clone()),
    ];
    for (source, bytes) in &payloads {
        host.asset_resources_mut()
            .register_client_source(child, source.clone(), bytes.clone())?;
    }
    place(
        &mut host,
        nested,
        vec![
            ComponentValue::CanvasDrawing(CanvasDrawing {
                source: drawing.uri.clone(),
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 32.0,
                y: 64.0,
                scale_x: 48.0,
                scale_y: 24.0,
                red: 0.0,
                green: 0.5,
                blue: 0.0,
                opacity: 0.5,
                ..Default::default()
            }),
        ],
    )?;
    place(
        &mut host,
        nested,
        vec![
            ComponentValue::CanvasText(CanvasText {
                text: "AOg0".into(),
                source: font.uri.clone(),
                font_size: 24.0,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 8.0,
                y: 12.0,
                red: 1.0,
                green: 1.0,
                blue: 0.0,
                ..Default::default()
            }),
        ],
    )?;
    let bitmap_entity = place(
        &mut host,
        nested,
        vec![
            ComponentValue::CanvasBitmap(CanvasBitmap {
                source: bitmap.uri.clone(),
                width: 40.0,
                height: 40.0,
                ..Default::default()
            }),
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 72.0,
                y: 52.0,
                opacity: 0.5,
                ..Default::default()
            }),
        ],
    )?;
    let anchor = attach(&mut host, root, nested, [0.0; 2], [1.28; 2], false)?;
    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::CanvasStyle(CanvasStyle {
                opacity: 0.5,
                ..Default::default()
            }),
        )],
    )?;
    let viewport = WorldViewport {
        width: 256,
        height: 256,
        device_pixel_ratio: 2.0,
    };
    host.set_root_output(root, viewport)?;
    let keys: Vec<_> = payloads
        .iter()
        .map(|(source, _)| host.asset_resources().find(source).unwrap())
        .collect();
    let progress_limit = payloads
        .iter()
        .map(|(_, bytes)| {
            bytes
                .len()
                .div_ceil(ipp_core::services::asset_management::STREAM_CAPACITY)
        })
        .sum::<usize>()
        + 32;
    for _ in 0..progress_limit {
        host.frame(0.0)?;
        let selected = host
            .root_output(root.world().id())
            .map(|(output, _, publication)| (output, publication));
        renderer.prepare(&mut host, selected)?;
        host.progress_assets();
        if keys.iter().all(|key| {
            host.asset_resources()
                .get(*key)
                .and_then(|resource| resource.data())
                .is_some_and(|data| data.graphics_ready() != Some(false))
        }) {
            break;
        }
    }
    assert!(keys.iter().all(|key| {
        host.asset_resources()
            .get(*key)
            .and_then(|resource| resource.data())
            .is_some_and(|data| data.graphics_ready() != Some(false))
    }));
    frame_at(renderer, &mut host, root, viewport, 0.0)?;
    let direct = capture()?;
    save(output, "canvas-assets-direct", &direct)?;
    let background = [10.0_f32, 14.0, 20.0].map(|encoded| {
        let encoded = encoded / 255.0;
        if encoded <= 0.04045 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        }
    });

    let green = [0.0, 0.5, 0.0];
    assert_color(
        &direct,
        64,
        128,
        std::array::from_fn(|axis| green[axis] * 0.25 + background[axis] * 0.75),
    );
    let alpha = (128.0 / 255.0) * 0.25;
    let badge_rgb = [255.0_f32, 160.0, 32.0].map(|encoded| {
        let encoded = encoded / 255.0;
        if encoded <= 0.04045 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        }
    });
    assert_color(
        &direct,
        184,
        144,
        std::array::from_fn(|axis| badge_rgb[axis] * alpha + background[axis] * (1.0 - alpha)),
    );
    let glyph_pixels = (16..180)
        .flat_map(|column| (16..100).map(move |row| (row * 256 + column) * 4))
        .filter(|offset| {
            direct[*offset] > 150 && direct[*offset + 1] > 150 && direct[*offset + 2] < 60
        })
        .count();
    assert!(glyph_pixels > 200, "font contours missing: {glyph_pixels}");
    for _ in 0..4 {
        frame_at(renderer, &mut host, root, viewport, 0.0)?;
    }

    let warm = capture()?;
    frame_at(renderer, &mut host, root, viewport, 0.0)?;
    assert_eq!(renderer.statistics().uploaded_bytes, 0);
    assert_eq!(capture()?, warm);
    apply(
        &mut host,
        parent,
        vec![Command::insert_value(
            EntityRef::Handle(anchor),
            ComponentValue::SurfaceCache(SurfaceCache {
                direct_distance: 0.0,
                resolution_scale: 1.0,
                max_refresh_hz: 1.0,
            }),
        )],
    )?;
    frame_at(renderer, &mut host, root, viewport, 0.0)?;
    let cached = capture()?;
    save(output, "canvas-assets-cached", &cached)?;
    assert_eq!(renderer.statistics().surface_cache_repaints, 1);
    for position in [[64, 128], [184, 144]] {
        let offset = (position[1] * 256 + position[0]) * 4;
        assert!(
            cached[offset..offset + 4]
                .iter()
                .zip(&warm[offset..offset + 4])
                .all(|(left, right)| left.abs_diff(*right) <= 2)
        );
    }

    let old_key = host.asset_resources().find(&bitmap).unwrap();
    host.asset_resources_mut()
        .release_client_source(child, &bitmap);
    let mut replacement = badge;
    for pixel in replacement[16..].as_chunks_mut::<4>().0 {
        pixel[..3].copy_from_slice(&[0, 0, 255]);
    }

    let replacement_source = source(2, 4);
    host.asset_resources_mut().register_client_source(
        child,
        replacement_source.clone(),
        replacement,
    )?;
    apply(
        &mut host,
        child,
        vec![Command::insert_value(
            EntityRef::Handle(bitmap_entity),
            ComponentValue::CanvasBitmap(CanvasBitmap {
                source: replacement_source.uri.clone(),
                width: 40.0,
                height: 40.0,
                ..Default::default()
            }),
        )],
    )?;
    let new_key = host.asset_resources().find(&replacement_source).unwrap();
    assert_ne!(old_key, new_key);
    frame_at(renderer, &mut host, root, viewport, 1.0)?;
    let replaced = capture()?;
    save(output, "canvas-assets-replacement", &replaced)?;
    assert_color(
        &replaced,
        184,
        144,
        std::array::from_fn(|axis| f32::from(axis == 2) * alpha + background[axis] * (1.0 - alpha)),
    );
    renderer.prepare(&mut host, None)?;
    renderer.unload_host(&mut host)?;
    Ok(())
}
