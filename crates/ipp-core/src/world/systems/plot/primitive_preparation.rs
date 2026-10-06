//! Shared text/shape/path preparation; algorithm geometry stays camera-independent.

use super::*;
use crate::ErrorReason;
use crate::services::asset_management::{AssetKey, formats::font::FontAsset};
use crate::systems::canvas::*;
use crate::text::{
    TextFont, TextLinePolicy, TextMaxWidth, TextMeasureRequest, TextOutcome, measure_text,
};
use std::sync::Arc;

pub(super) fn prepare(
    target: CanvasTarget,
    items: &[PlotPrimitive],
    font: Option<(AssetKey, &FontAsset)>,
) -> Result<Arc<[CanvasPrimitive]>, ErrorReason> {
    let mut output = Vec::with_capacity(items.len());
    for item in items {
        let style = CanvasPrimitiveStyle {
            identity: CanvasPrimitiveId {
                target,
                part: CanvasPart::Plot(item.part),
            },
            position: item.position,
            scale: [1.0; 2],
            color: item.color,
            opacity: 1.0,
            clip: [-f32::MAX, -f32::MAX, f32::MAX, f32::MAX],
            layer: 0,
        };
        output.push(match &item.kind {
            PlotPrimitiveKind::Box {
                size,
                border_width,
                border_color,
            } => CanvasPrimitive::Box {
                // Box fill is intrinsic paint, not a tint of its independent
                // outline. Canvas composition still applies inherited tint/opacity.
                style: CanvasPrimitiveStyle {
                    color: [1.0; 4],
                    ..style
                },
                size: *size,
                corner_radius: [0.0; 2],
                border_width: *border_width,
                border_color: *border_color,
                fill: CanvasShapeFill::Solid(item.color),
                glow: None,
                shape: CanvasBoxShape::RECT,
            },
            PlotPrimitiveKind::Path(path) => CanvasPrimitive::Path {
                style,
                path: path.clone(),
            },
            PlotPrimitiveKind::Text {
                text,
                font_size,
            } => {
                let Some((key, font)) = font else {
                    // An unauthored font deliberately omits labels; an authored pending
                    // font is rejected by the caller before preparation succeeds.
                    continue;
                };
                let request = TextMeasureRequest::new(
                    text,
                    TextFont::Ready {
                        key,
                        font,
                    },
                    *font_size,
                    TextLinePolicy::Multiline,
                    TextMaxWidth::Unbounded,
                )
                .map_err(|_| ErrorReason::InvalidValue)?;
                let TextOutcome::Measured(layout) = measure_text(&request) else {
                    return Err(ErrorReason::InvalidAsset);
                };
                CanvasPrimitive::Glyphs {
                    style,
                    font: key,
                    font_size: *font_size,
                    glyphs: layout
                        .glyphs
                        .iter()
                        .map(|glyph| CanvasGlyph {
                            glyph_id: glyph.glyph_id,
                            position: glyph.position.map(|value| value * *font_size),
                            color: None,
                        })
                        .collect::<Vec<_>>()
                        .into(),
                }
            }
        });
    }
    Ok(output.into())
}

/// Exact normalized ink/panel rectangles; connectors are independently retained.
pub(super) fn bounds(items: &[CanvasPrimitive], font: Option<&FontAsset>) -> Option<[f32; 4]> {
    let mut result: Option<[f32; 4]> = None;
    let mut include = |bounds: [f32; 4], style: &CanvasPrimitiveStyle| {
        let x = [
            bounds[0] * style.scale[0] + style.position[0],
            bounds[2] * style.scale[0] + style.position[0],
        ];
        let y = [
            bounds[1] * style.scale[1] + style.position[1],
            bounds[3] * style.scale[1] + style.position[1],
        ];
        let bounds = [
            x[0].min(x[1]),
            y[0].min(y[1]),
            x[0].max(x[1]),
            y[0].max(y[1]),
        ];
        if bounds[0] >= bounds[2] || bounds[1] >= bounds[3] {
            return;
        }
        let target = result.get_or_insert(bounds);
        target[0] = target[0].min(bounds[0]);
        target[1] = target[1].min(bounds[1]);
        target[2] = target[2].max(bounds[2]);
        target[3] = target[3].max(bounds[3]);
    };
    for item in items {
        match item {
            CanvasPrimitive::Glyphs {
                style,
                glyphs,
                font_size,
                ..
            } => {
                let Some(font) = font else {
                    continue;
                };
                let unit = *font_size / font.units_per_em() as f32;
                for glyph in glyphs.iter() {
                    let Some(outline) = font.glyph(glyph.glyph_id) else {
                        continue;
                    };
                    if outline.contours.is_empty() {
                        continue;
                    }
                    // Font source is Y-up; shared glyph rendering is top-left Y-down.
                    include(
                        [
                            glyph.position[0] + outline.bounds[0] * unit,
                            glyph.position[1] - outline.bounds[3] * unit,
                            glyph.position[0] + outline.bounds[2] * unit,
                            glyph.position[1] - outline.bounds[1] * unit,
                        ],
                        style,
                    );
                }
            }
            CanvasPrimitive::Box {
                style,
                size,
                ..
            } => include([0.0, 0.0, size[0], size[1]], style),
            CanvasPrimitive::Path {
                style,
                path,
            } => include(path.bounds, style),
            _ => {}
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_panel_fill_does_not_dim_its_independent_cyan_outline() {
        let fill = [0.002, 0.016, 0.025, 0.95];
        let outline = [0.0, 0.8, 1.0, 1.0];
        let target = CanvasTarget {
            entity: crate::EntityId::from_bits((1 << 32) | 1),
            component: crate::ComponentValue::PLOT_GRID_BARS3D,
            incarnation: 1,
        };
        let items = [PlotPrimitive {
            part: 0,
            position: [0.0; 2],
            color: fill,
            kind: PlotPrimitiveKind::Box {
                size: [2.0, 1.0],
                border_width: 0.015,
                border_color: outline,
            },
        }];
        let prepared = prepare(target, &items, None).unwrap();
        let CanvasPrimitive::Box {
            style,
            fill: CanvasShapeFill::Solid(color),
            border_color,
            ..
        } = &prepared[0]
        else {
            panic!("prepared panel")
        };
        assert_eq!(*color, fill);
        for channel in 0..4 {
            assert_eq!(
                border_color[channel] * style.color[channel],
                outline[channel]
            );
        }
    }

    #[test]
    fn prepared_ink_bounds_normalize_font_y_and_signed_visual_scale() {
        // One asymmetric triangular glyph in a real portable font payload.
        let mut bytes = b"IPPF".to_vec();
        for value in [1_u32, 1000] {
            bytes.extend(value.to_le_bytes());
        }
        for value in [800.0_f32, -200.0, 0.0] {
            bytes.extend(value.to_le_bytes());
        }
        for value in [1_u32, 1, 0] {
            bytes.extend(value.to_le_bytes());
        }
        for value in [500.0_f32, 100.0, 100.0, -200.0, 500.0, 700.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(1_u32.to_le_bytes());
        for value in [100.0_f32, -200.0] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(2_u32.to_le_bytes());
        for point in [[500.0_f32, 700.0], [100.0, 700.0]] {
            bytes.extend([0_u8; 4]);
            for value in point {
                bytes.extend(value.to_le_bytes());
            }
        }
        for value in [65_u32, 0] {
            bytes.extend(value.to_le_bytes());
        }
        let font = FontAsset::decode(&bytes).unwrap();
        let item = CanvasPrimitive::Glyphs {
            style: CanvasPrimitiveStyle {
                identity: CanvasPrimitiveId {
                    target: CanvasTarget {
                        entity: crate::EntityId::from_bits((1 << 32) | 1),
                        component: crate::ComponentValue::PLOT_POINTS3D,
                        incarnation: 1,
                    },
                    part: CanvasPart::Plot(0),
                },
                position: [3.0, 4.0],
                scale: [-2.0, 3.0],
                color: [1.0; 4],
                opacity: 1.0,
                clip: [-f32::MAX, -f32::MAX, f32::MAX, f32::MAX],
                layer: 0,
            },
            font: AssetKey::from_u64(1),
            font_size: 2.0,
            glyphs: Arc::from([CanvasGlyph {
                glyph_id: 0,
                position: [10.0, 20.0],
                color: None,
            }]),
        };
        let result = bounds(&[item], Some(&font)).unwrap();
        for (actual, expected) in result.into_iter().zip([-19.0, 59.8, -17.4, 65.2]) {
            assert!((actual - expected).abs() < 0.00001);
        }
    }
}
