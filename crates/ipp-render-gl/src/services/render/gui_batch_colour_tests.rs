//! Tests for the colour-field fills, the checker and custom paints: their record lanes
//! and paints, and the colour, checker and paint-input evaluation of
//! `surface_gui.frag`.
//!
//! [`shader_fill`], [`shader_checker`] and [`shader_paint_inputs`] transliterate the
//! shader's `colour_field`, `gradient_color`, `checker_color` and `paint_fill` line
//! for line and read only the generated lanes, so comparing them with an independent HSV model, sRGB transfer function
//! and brute-force checker area checks the packing and the shader's formulas together.

use super::{
    GUI_FILL_HUE, GUI_FILL_PAINT, GUI_FILL_SATURATION_VALUE, GUI_PAINT_ARC, GUI_PAINT_BLOCK_STRIDE,
    GUI_PAINT_CHECKER, GUI_PAINT_STROKE, GuiShapeRecord, generate_box_records, hash_box_inputs,
    hash_painted_box_inputs, painted_box_records,
};
use crate::services::render::canvas_paint::GuiPaintLanes;
use ipp_core::systems::canvas::{
    CanvasBoxShape, CanvasClip, CanvasPart, CanvasPrimitiveId, CanvasPrimitiveStyle,
    CanvasShapeChecker, CanvasShapeFill, CanvasTarget,
};

const CLIP: CanvasClip = [-1000.0, -1000.0, 1000.0, 1000.0];

fn style(position: [f32; 2], scale: [f32; 2]) -> CanvasPrimitiveStyle {
    CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: CanvasTarget {
                entity: ipp_core::EntityId::from_bits(1),
                component: ipp_core::ComponentValue::GUI_SKIN,
                incarnation: 1,
            },
            part: CanvasPart::Background,
        },
        position,
        scale,
        color: [1.0; 4],
        opacity: 1.0,
        clip: CLIP,
        layer: 0,
    }
}

fn checker_rect(checker: Option<CanvasShapeChecker>, accent: [f32; 4]) -> CanvasBoxShape {
    CanvasBoxShape::Rect {
        corner_cut: [0.0; 4],
        corner_accent: accent,
        corner_accent_width: 3.0,
        checker,
    }
}

fn records(
    style: &CanvasPrimitiveStyle,
    size: [f32; 2],
    fill: CanvasShapeFill,
    shape: CanvasBoxShape,
) -> Vec<GuiShapeRecord> {
    generate_box_records(
        style,
        &size,
        &[0.0; 2],
        1.0,
        &[0.0, 0.0, 0.0, 1.0],
        &fill,
        None,
        &shape,
        CLIP,
    )
}

fn sv(hue: f32) -> CanvasShapeFill {
    CanvasShapeFill::SaturationValue {
        hue,
    }
}

const GREYS: CanvasShapeChecker = CanvasShapeChecker {
    size: 6.0,
    colors: [[0.6, 0.6, 0.6, 1.0], [0.3, 0.3, 0.3, 1.0]],
};

// ---- Independent references ----

/// The textbook HSV model in sRGB encoding: hue sector, then the three products.
fn hsv_to_srgb(hue: f64, saturation: f64, value: f64) -> [f64; 3] {
    let sector = hue.rem_euclid(1.0) * 6.0;
    let index = sector.floor();
    let f = sector - index;
    let p = value * (1.0 - saturation);
    let q = value * (1.0 - saturation * f);
    let t = value * (1.0 - saturation * (1.0 - f));
    match index as u32 % 6 {
        0 => [value, t, p],
        1 => [q, value, p],
        2 => [p, value, t],
        3 => [p, q, value],
        4 => [t, p, value],
        _ => [value, p, q],
    }
}

/// The sRGB transfer function, as the target's store and the present pass apply it.
fn encode(linear: f64) -> f64 {
    if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

fn decode(encoded: f64) -> f64 {
    if encoded <= 0.040_45 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

// ---- Line-for-line mirror of surface_gui.frag ----

fn fract(value: f32) -> f32 {
    value - value.floor()
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn gradient_axis(record: &GuiShapeRecord, local: [f32; 2]) -> f32 {
    let [x0, y0, x1, y1] = record.gradient_coords;
    let dir = [x1 - x0, y1 - y0];
    let len_sq = dir[0] * dir[0] + dir[1] * dir[1];
    if len_sq > 1e-12 {
        (((local[0] - x0) * dir[0] + (local[1] - y0) * dir[1]) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn hue_color(hue: f32) -> [f32; 3] {
    [0.0, 2.0 / 3.0, 1.0 / 3.0]
        .map(|offset: f32| ((fract(hue + offset) * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0))
}

fn srgb_to_linear(encoded: [f32; 3]) -> [f32; 3] {
    encoded.map(|channel| {
        if channel <= 0.040_45 {
            channel / 12.92
        } else {
            ((channel + 0.055) / 1.055).powf(2.4)
        }
    })
}

fn colour_field(record: &GuiShapeRecord, fill_type: f32, local: [f32; 2]) -> [f32; 4] {
    let encoded = if fill_type < GUI_FILL_HUE + 0.5 {
        hue_color(gradient_axis(record, local))
    } else {
        let [x0, y0, x1, y1] = record.gradient_coords;
        let size = [x1 - x0, y1 - y0];
        let fraction = |offset: f32, extent: f32| {
            (offset
                / (extent
                    + if extent == 0.0 {
                        1.0
                    } else {
                        0.0
                    }))
            .clamp(0.0, 1.0)
        };
        let s = fraction(local[0] - x0, size[0]);
        let v = fraction(local[1] - y0, size[1]);
        hue_color(record.color1[0]).map(|channel| (1.0 + (channel - 1.0) * s) * (1.0 - v))
    };
    let linear = srgb_to_linear(encoded);
    [
        linear[0] * record.color0[0],
        linear[1] * record.color0[1],
        linear[2] * record.color0[2],
        record.color0[3],
    ]
}

fn gradient_color(record: &GuiShapeRecord, t: f32) -> [f32; 4] {
    let premultiplied = |color: [f32; 4]| {
        [
            color[0] * color[3],
            color[1] * color[3],
            color[2] * color[3],
            color[3],
        ]
    };
    let start = premultiplied(record.color0);
    let end = premultiplied(record.color1);
    let color: [f32; 4] = std::array::from_fn(|lane| start[lane] + (end[lane] - start[lane]) * t);
    if color[3] > 0.0 {
        [
            color[0] / color[3],
            color[1] / color[3],
            color[2] / color[3],
            color[3],
        ]
    } else {
        [0.0; 4]
    }
}

/// Straight fill colour of `record` at a Surface `position`, as `shape_color`
/// evaluates its linear gradient and colour fields.
fn shader_fill(record: &GuiShapeRecord, position: [f32; 2]) -> [f32; 4] {
    let paint = record.material_params[0];
    let fill_type = paint % GUI_PAINT_CHECKER;
    let local = [
        position[0] - record.placement[0],
        position[1] - record.placement[1],
    ];
    if fill_type > 0.5 && fill_type < 1.5 {
        gradient_color(record, gradient_axis(record, local))
    } else if fill_type > 2.5 {
        colour_field(record, fill_type, local)
    } else {
        record.color0
    }
}

fn checker_wave_integral(x: f32) -> f32 {
    1.0 - (2.0 * fract(x * 0.5) - 1.0).abs()
}

fn checker_rgb(lane: f32) -> [f32; 3] {
    let bits = lane as u32;
    [16, 8, 0].map(|shift| {
        let root = ((bits >> shift) & 0xFF) as f32 / 255.0;
        root * root
    })
}

/// Premultiplied checker colour of `record` at a Surface `position` whose pixel
/// footprint moves `dx` and `dy` across the Surface, as `checker_color` evaluates it.
fn shader_checker(
    record: &GuiShapeRecord,
    position: [f32; 2],
    dx: [f32; 2],
    dy: [f32; 2],
) -> [f32; 4] {
    let sign = |value: f32| {
        if value > 0.0 {
            1.0
        } else if value < 0.0 {
            -1.0
        } else {
            0.0
        }
    };
    let own: [f32; 2] = std::array::from_fn(|axis| {
        (position[axis] - record.placement[axis]) * sign(record.placement[axis + 2])
    });
    let own_dx: [f32; 2] = std::array::from_fn(|axis| dx[axis] * sign(record.placement[axis + 2]));
    let own_dy: [f32; 2] = std::array::from_fn(|axis| dy[axis] * sign(record.placement[axis + 2]));
    let cell = record.corner_accent[0];
    let wave: [f32; 2] = std::array::from_fn(|axis| {
        let cells = own[axis] / cell;
        let width = ((own_dx[axis].abs() + own_dy[axis].abs()) / cell).max(1.0 / 65536.0);
        let wave = (checker_wave_integral(cells + 0.5 * width)
            - checker_wave_integral(cells - 0.5 * width))
            / width;
        wave * (1.0 - smoothstep(0.5, 1.0, width))
    });
    let second = 0.5 - 0.5 * wave[0] * wave[1];
    let alphas = record.corner_accent[3] as u32;
    let alpha = [(alphas >> 8) as f32 / 255.0, (alphas & 0xFF) as f32 / 255.0];
    let first_rgb = checker_rgb(record.corner_accent[1]);
    let second_rgb = checker_rgb(record.corner_accent[2]);
    let first = [
        first_rgb[0] * alpha[0],
        first_rgb[1] * alpha[0],
        first_rgb[2] * alpha[0],
        alpha[0],
    ];
    let other = [
        second_rgb[0] * alpha[1],
        second_rgb[1] * alpha[1],
        second_rgb[2] * alpha[1],
        alpha[1],
    ];
    std::array::from_fn(|lane| first[lane] + (other[lane] - first[lane]) * second)
}

/// Displayed sRGB levels of an opaque linear colour, before 8-bit rounding.
fn displayed(linear: [f32; 4]) -> [f64; 3] {
    std::array::from_fn(|channel| encode(f64::from(linear[channel])) * 255.0)
}

fn max_level_error(actual: [f64; 3], expected: [f64; 3]) -> f64 {
    (0..3)
        .map(|channel| (actual[channel] - expected[channel] * 255.0).abs())
        .fold(0.0, f64::max)
}

// ---- Lanes and paints ----

#[test]
fn colour_fields_pack_their_axis_or_rectangle_hue_and_tint() {
    let mut tinted = style([10.0, 20.0], [2.0, -0.5]);
    tinted.color = [0.5, 1.0, 1.0, 0.8];
    tinted.opacity = 0.5;
    let hue = CanvasShapeFill::Hue {
        start: [0.0, 96.0],
        end: [4.0, 0.0],
    };
    for record in records(&tinted, [24.0, 96.0], hue, CanvasBoxShape::RECT) {
        assert_eq!(record.material_params[0], GUI_FILL_HUE);
        // The axis scales with the primitive like a linear gradient's.
        assert_eq!(record.gradient_coords, [0.0, -48.0, 8.0, 0.0]);
        // Opaque model colours under the tint and both opacities.
        assert_eq!(record.color0, [0.5, 1.0, 1.0, 0.4]);
        assert_eq!(record.color1, [0.0; 4]);
    }

    for (hue, lane) in [(0.55, 0.55), (2.25, 0.25), (-0.25, 0.75), (f32::NAN, 0.0)] {
        for record in records(&tinted, [96.0, 64.0], sv(hue), CanvasBoxShape::RECT) {
            assert_eq!(record.material_params[0], GUI_FILL_SATURATION_VALUE);
            // The placed part rectangle from its origin, signed when mirrored.
            assert_eq!(record.gradient_coords, [0.0, 0.0, 192.0, -32.0]);
            assert_eq!(record.color0, [0.5, 1.0, 1.0, 0.4]);
            assert_eq!(record.color1, [lane, 0.0, 0.0, 0.0], "hue {hue}");
        }
    }
}

#[test]
fn a_checker_takes_the_accent_lanes_and_covers_the_interior() {
    let mut tinted = style([0.0, 0.0], [2.0, 1.0]);
    tinted.color = [1.0, 0.25, 1.0, 1.0];
    tinted.opacity = 0.5;
    let checker = CanvasShapeChecker {
        size: 3.0,
        colors: [[0.6, 0.6, 0.6, 1.0], [0.04, 1.0, 0.0, 0.2]],
    };
    let transparent = CanvasShapeFill::Solid([0.0; 4]);
    let shape = checker_rect(Some(checker), [8.0; 4]);
    let generated = records(&tinted, [40.0, 40.0], transparent, shape);
    // A transparent fill over a checker still paints its interior: one quad.
    assert_eq!(generated.len(), 1);
    let byte = |value: f32| (value * 255.0).round();
    for record in &generated {
        assert_eq!(record.material_params[0], GUI_PAINT_CHECKER);
        // The accent width and spans give way to the checker's lanes.
        assert_eq!(record.shape[3], 0.0);
        let [cell, first, second, alphas] = record.corner_accent;
        assert_eq!(cell, 6.0, "the cell scales by the larger scale");
        let root = |channel: f32| byte(channel.sqrt());
        assert_eq!(
            first,
            root(0.6) * 65536.0 + root(0.15) * 256.0 + root(0.6),
            "tinted square-root bytes"
        );
        assert_eq!(second, root(0.04) * 65536.0 + root(0.25) * 256.0);
        assert_eq!(alphas, byte(0.5) * 256.0 + byte(0.1));
    }

    // Solid boxes keep paint zero and their accents without a checker, and a
    // checker without a usable cell paints none.
    for size in [0.0, f32::NAN, f32::INFINITY] {
        let none = records(
            &style([0.0; 2], [1.0; 2]),
            [40.0, 40.0],
            CanvasShapeFill::Solid([1.0; 4]),
            checker_rect(
                Some(CanvasShapeChecker {
                    size,
                    ..GREYS
                }),
                [8.0; 4],
            ),
        );
        for record in none {
            assert_eq!(record.material_params[0], 0.0);
            assert_eq!(record.corner_accent, [8.0; 4]);
            assert_eq!(record.shape[3], 3.0);
        }
    }
}

#[test]
fn checker_colours_return_within_one_srgb_step() {
    let mut worst = 0.0f64;
    for level in 0..=255u32 {
        let linear = decode(f64::from(level) / 255.0) as f32;
        let checker = CanvasShapeChecker {
            size: 4.0,
            colors: [[linear; 4], [0.0, 0.0, 0.0, 1.0]],
        };
        let record = records(
            &style([0.0; 2], [1.0; 2]),
            [8.0, 8.0],
            CanvasShapeFill::Solid([0.0; 4]),
            checker_rect(Some(checker), [0.0; 4]),
        )[0];
        let rgb = checker_rgb(record.corner_accent[1]);
        for channel in rgb {
            worst = worst.max((encode(f64::from(channel)) * 255.0 - f64::from(level)).abs());
        }
    }
    assert!(worst < 0.7, "worst error {worst} sRGB levels");
}

#[test]
fn colour_fields_and_the_checker_change_the_retained_hash() {
    let style = style([0.0; 2], [1.0; 2]);
    let hash = |fill: CanvasShapeFill, shape: CanvasBoxShape| {
        hash_box_inputs(
            &style,
            &[24.0, 96.0],
            &[0.0; 2],
            1.0,
            &[1.0; 4],
            &fill,
            None,
            &shape,
            CLIP,
        )
    };
    let hue = |end: f32| CanvasShapeFill::Hue {
        start: [0.0, 96.0],
        end: [0.0, end],
    };
    let base = hash(sv(0.25), CanvasBoxShape::RECT);
    let variants = [
        hash(sv(0.5), CanvasBoxShape::RECT),
        hash(hue(0.0), CanvasBoxShape::RECT),
        hash(CanvasShapeFill::Solid([1.0; 4]), CanvasBoxShape::RECT),
        hash(sv(0.25), checker_rect(Some(GREYS), [0.0; 4])),
        hash(
            sv(0.25),
            checker_rect(
                Some(CanvasShapeChecker {
                    size: 7.0,
                    ..GREYS
                }),
                [0.0; 4],
            ),
        ),
        hash(
            sv(0.25),
            checker_rect(
                Some(CanvasShapeChecker {
                    colors: [GREYS.colors[1], GREYS.colors[0]],
                    ..GREYS
                }),
                [0.0; 4],
            ),
        ),
    ];
    assert_ne!(
        hash(hue(0.0), CanvasBoxShape::RECT),
        hash(hue(1.0), CanvasBoxShape::RECT)
    );
    for (index, variant) in variants.into_iter().enumerate() {
        assert_ne!(variant, base, "variant {index}");
    }
}

// ---- Colour model ----

#[test]
fn saturation_value_fields_display_the_hsv_colour_of_every_point() {
    let mut worst = 0.0f64;
    // Plain, offset and scaled, and mirrored on both axes.
    for (position, scale) in [
        ([0.0, 0.0], [1.0, 1.0]),
        ([30.0, -12.0], [2.0, 0.5]),
        ([200.0, 150.0], [-1.0, -1.5]),
    ] {
        let size = [96.0f32, 80.0];
        for hue in [
            0.0f32,
            0.1,
            1.0 / 6.0,
            0.3,
            0.55,
            2.0 / 3.0,
            0.8,
            0.95,
            1.0,
            -0.4,
        ] {
            let record = records(&style(position, scale), size, sv(hue), CanvasBoxShape::RECT)[0];
            for row in 0..=16 {
                for column in 0..=16 {
                    let saturation = f64::from(column) / 16.0;
                    let value = 1.0 - f64::from(row) / 16.0;
                    // The point in the box's own orientation, then placed.
                    let point = [
                        position[0] + scale[0] * size[0] * column as f32 / 16.0,
                        position[1] + scale[1] * size[1] * row as f32 / 16.0,
                    ];
                    let expected = hsv_to_srgb(f64::from(hue), saturation, value);
                    let error = max_level_error(displayed(shader_fill(&record, point)), expected);
                    worst = worst.max(error);
                }
            }
        }
    }
    assert!(worst < 0.05, "worst error {worst} sRGB levels");

    // Corners: white, the pure hue, black.
    let record = records(
        &style([0.0; 2], [1.0; 2]),
        [10.0, 10.0],
        sv(0.55),
        CanvasBoxShape::RECT,
    )[0];
    let pure = hsv_to_srgb(0.55, 1.0, 1.0);
    assert!(max_level_error(displayed(shader_fill(&record, [0.0, 0.0])), [1.0; 3]) < 0.01);
    assert!(max_level_error(displayed(shader_fill(&record, [10.0, 0.0])), pure) < 0.05);
    assert!(max_level_error(displayed(shader_fill(&record, [3.0, 10.0])), [0.0; 3]) < 0.01);

    // The linear-light blend a gradient would give is far from the model: halfway
    // from white to cyan it shows sRGB 188 where HSV gives 128.
    let linear_mid = encode(0.5) * 255.0;
    assert!((linear_mid - 187.5).abs() < 0.5);
    let cyan = records(
        &style([0.0; 2], [1.0; 2]),
        [10.0, 10.0],
        sv(0.5),
        CanvasBoxShape::RECT,
    )[0];
    let half = displayed(shader_fill(&cyan, [5.0, 0.0]));
    assert!((half[0] - 127.5).abs() < 0.05, "{half:?}");
}

#[test]
fn hue_rails_display_the_hue_circle_along_their_axis() {
    let mut worst = 0.0f64;
    // A vertical rail with red at its bottom, a horizontal one, and a mirrored one.
    for (position, scale, size, start, end) in [
        (
            [0.0, 0.0],
            [1.0, 1.0],
            [24.0, 144.0],
            [0.0, 144.0],
            [0.0, 0.0],
        ),
        (
            [5.0, 7.0],
            [1.0, 1.0],
            [144.0, 24.0],
            [0.0, 0.0],
            [144.0, 0.0],
        ),
        (
            [300.0, 9.0],
            [-2.0, 1.0],
            [144.0, 24.0],
            [0.0, 0.0],
            [144.0, 0.0],
        ),
    ] {
        let fill = CanvasShapeFill::Hue {
            start,
            end,
        };
        let record = records(&style(position, scale), size, fill, CanvasBoxShape::RECT)[0];
        for step in 0..=96 {
            let t = step as f32 / 96.0;
            let local = [
                start[0] + (end[0] - start[0]) * t,
                start[1] + (end[1] - start[1]) * t,
            ];
            // Across the rail the hue stays.
            for across in [0.25f32, 0.75] {
                let offset = if start[0] == end[0] {
                    [size[0] * across - local[0], 0.0]
                } else {
                    [0.0, size[1] * across - local[1]]
                };
                let point = [
                    position[0] + scale[0] * (local[0] + offset[0]),
                    position[1] + scale[1] * (local[1] + offset[1]),
                ];
                let expected = hsv_to_srgb(f64::from(t), 1.0, 1.0);
                worst = worst.max(max_level_error(
                    displayed(shader_fill(&record, point)),
                    expected,
                ));
            }
        }
        // Beyond either end the colour is constant: red.
        let before = [
            position[0] + scale[0] * (start[0] - (end[0] - start[0]) * 0.1),
            position[1] + scale[1] * (start[1] - (end[1] - start[1]) * 0.1),
        ];
        assert!(max_level_error(displayed(shader_fill(&record, before)), [1.0, 0.0, 0.0]) < 0.01);
    }
    assert!(worst < 0.05, "worst error {worst} sRGB levels");
}

#[test]
fn strokes_and_arcs_keep_colour_fields_on_their_part_rectangle_and_paint_no_checker() {
    let part = style([10.0, 20.0], [1.0, 1.0]);
    let size = [64.0, 64.0];
    let fills = [
        sv(0.3),
        CanvasShapeFill::Hue {
            start: [0.0, 64.0],
            end: [0.0, 0.0],
        },
    ];
    let shapes = [
        (
            CanvasBoxShape::Stroke {
                segments: [[0.1, 0.2, 0.9, 0.7], [0.0; 4]],
            },
            GUI_PAINT_STROKE,
        ),
        (
            CanvasBoxShape::Arc {
                start: 0.6,
                sweep: 0.5,
                dashes: 0.0,
                dash_duty: 1.0,
            },
            GUI_PAINT_ARC,
        ),
    ];
    for fill in fills {
        let reference = records(&part, size, fill, CanvasBoxShape::RECT)[0];
        for (shape, offset) in shapes {
            let shaped = records(&part, size, fill, shape)[0];
            let fill_type = reference.material_params[0];
            assert_eq!(shaped.material_params[0], fill_type + offset);
            assert_ne!(shaped.placement, reference.placement, "{shape:?}");
            // Re-anchored at the shape's own bounds, the field gives every Surface
            // point the colour the part rectangle gives it.
            for point in [[20.0, 30.0], [42.0, 50.0], [70.0, 80.0], [15.0, 79.0]] {
                let expected = shader_fill(&reference, point);
                let actual = shader_fill(&shaped, point);
                for lane in 0..4 {
                    assert!(
                        (actual[lane] - expected[lane]).abs() < 1e-5,
                        "{shape:?} at {point:?}"
                    );
                }
            }
        }
    }

    // The arc's and stroke's lanes hold their geometry, never a checker.
    let checkered = checker_rect(Some(GREYS), [0.0; 4]);
    let boxed = records(&part, size, sv(0.3), checkered)[0];
    assert_eq!(
        boxed.material_params[0],
        GUI_FILL_SATURATION_VALUE + GUI_PAINT_CHECKER
    );
    for (shape, offset) in shapes {
        let shaped = records(&part, size, sv(0.3), shape)[0];
        assert_eq!(
            shaped.material_params[0],
            GUI_FILL_SATURATION_VALUE + offset
        );
    }
}

// ---- Alpha over the checker ----

#[test]
fn an_alpha_ramp_is_linear_coverage_of_one_colour() {
    // A colour from transparent at the bottom to opaque at the top: alpha equals the
    // fraction along the rail and the colour stays the stop's.
    let color = [0.1, 0.8, 1.0];
    let fill = CanvasShapeFill::LinearGradient {
        start: [0.0, 100.0],
        end: [0.0, 0.0],
        start_color: [color[0], color[1], color[2], 0.0],
        end_color: [color[0], color[1], color[2], 1.0],
    };
    let record = records(
        &style([0.0; 2], [1.0; 2]),
        [20.0, 100.0],
        fill,
        CanvasBoxShape::RECT,
    )[0];
    for step in 1..=10 {
        let alpha = step as f32 / 10.0;
        let straight = shader_fill(&record, [10.0, 100.0 * (1.0 - alpha)]);
        assert!((straight[3] - alpha).abs() < 1e-6);
        for channel in 0..3 {
            assert!((straight[channel] - color[channel]).abs() < 1e-5);
        }
    }

    // Composited over black in linear light, 50% white displays as sRGB 188, not
    // the 128 of blending encoded values.
    let shown = encode(0.5) * 255.0;
    assert!((shown - 187.5).abs() < 0.5, "{shown}");
}

// ---- Checker filtering ----

/// Brute-force premultiplied checker over an axis-aligned footprint: the mean of a
/// dense grid of hard-edged samples from the authored colours.
fn reference_checker(
    cell: f64,
    colors: [[f64; 4]; 2],
    center: [f64; 2],
    width: [f64; 2],
) -> [f64; 4] {
    const SAMPLES: usize = 128;
    let mut sum = [0.0; 4];
    for row in 0..SAMPLES {
        for column in 0..SAMPLES {
            let x = center[0] + width[0] * ((column as f64 + 0.5) / SAMPLES as f64 - 0.5);
            let y = center[1] + width[1] * ((row as f64 + 0.5) / SAMPLES as f64 - 0.5);
            let parity = ((x / cell).floor() + (y / cell).floor()).rem_euclid(2.0) as usize;
            let color = colors[parity];
            for lane in 0..3 {
                sum[lane] += color[lane] * color[3];
            }
            sum[3] += color[3];
        }
    }
    sum.map(|lane| lane / (SAMPLES * SAMPLES) as f64)
}

/// Authored colours after the packing's rounding, so the filter is compared alone.
fn packed_colors(record: &GuiShapeRecord) -> [[f64; 4]; 2] {
    let alphas = record.corner_accent[3] as u32;
    [1, 2].map(|lane| {
        let rgb = checker_rgb(record.corner_accent[lane]);
        let alpha = if lane == 1 {
            alphas >> 8
        } else {
            alphas & 0xFF
        } as f64
            / 255.0;
        [
            f64::from(rgb[0]),
            f64::from(rgb[1]),
            f64::from(rgb[2]),
            alpha,
        ]
    })
}

#[test]
fn the_checker_box_filters_its_cells_and_fades_to_their_mean_when_minified() {
    let colors = [[0.6, 0.6, 0.6, 1.0], [0.05, 0.3, 0.9, 0.5]];
    let checker = CanvasShapeChecker {
        size: 4.0,
        colors,
    };
    for (position, scale) in [([0.0, 0.0], [1.0, 1.0]), ([50.0, 40.0], [-1.0, 1.0])] {
        let record = records(
            &style(position, scale),
            [32.0, 32.0],
            CanvasShapeFill::Solid([0.0; 4]),
            checker_rect(Some(checker), [0.0; 4]),
        )[0];
        let packed = packed_colors(&record);
        let own = |point: [f32; 2]| {
            [
                f64::from((point[0] - position[0]) * scale[0]),
                f64::from((point[1] - position[1]) * scale[1]),
            ]
        };

        // Cell centres take their exact colours: the corner cell the first.
        for (cell, expected) in [([0, 0], 0), ([1, 0], 1), ([0, 1], 1), ([3, 5], 0)] {
            let point = [
                position[0] + scale[0] * (cell[0] as f32 + 0.5) * 4.0,
                position[1] + scale[1] * (cell[1] as f32 + 0.5) * 4.0,
            ];
            let color = shader_checker(&record, point, [0.25 * scale[0], 0.0], [0.0, 0.25]);
            let [r, g, b, a] = packed[expected];
            for (lane, value) in [r * a, g * a, b * a, a].into_iter().enumerate() {
                assert!(
                    (f64::from(color[lane]) - value).abs() < 1e-6,
                    "cell {cell:?}"
                );
            }
        }

        // Footprints under half a cell match the brute-force area, edges included.
        for (point, footprint) in [
            ([4.1, 1.0], [0.5, 0.5]),
            ([4.0, 4.0], [1.0, 1.0]),
            ([7.9, 12.3], [1.9, 0.3]),
            ([10.0, 2.0], [0.2, 1.8]),
        ] {
            let placed = [
                position[0] + scale[0] * point[0],
                position[1] + scale[1] * point[1],
            ];
            let color = shader_checker(&record, placed, [footprint[0], 0.0], [0.0, footprint[1]]);
            let expected = reference_checker(
                4.0,
                packed,
                own(placed),
                [f64::from(footprint[0]), f64::from(footprint[1])],
            );
            for lane in 0..4 {
                assert!(
                    (f64::from(color[lane]) - expected[lane]).abs() < 0.02,
                    "{point:?} {footprint:?}: {color:?} vs {expected:?}"
                );
            }
        }

        // A footprint of a cell or more along either axis, as on a minified or
        // oblique Surface, is the colours' mean wherever it falls.
        let mean: [f64; 4] = std::array::from_fn(|lane| {
            let [first, second] = packed;
            if lane < 3 {
                (first[lane] * first[3] + second[lane] * second[3]) / 2.0
            } else {
                (first[3] + second[3]) / 2.0
            }
        });
        for (dx, dy) in [
            ([4.0, 0.0], [0.0, 4.0]),
            ([9.0, 0.0], [0.0, 0.3]),
            ([2.9, 2.9], [-0.1, 1.2]),
            ([0.0, 0.2], [5.0, 0.0]),
        ] {
            for point in [[1.0, 1.0], [5.3, 17.7], [-3.0, 30.0]] {
                let color = shader_checker(&record, point, dx, dy);
                for lane in 0..4 {
                    assert!(
                        (f64::from(color[lane]) - mean[lane]).abs() < 1e-5,
                        "{dx:?} {dy:?}"
                    );
                }
            }
        }
    }
}

/// The arguments `paint_fill` passes to the paint dispatch at a Surface `position`
/// whose distance from the contour is `outer`, and the opacity it applies:
/// `(slot, block, position, size, color, edge, opacity)`.
#[allow(clippy::type_complexity)]
fn shader_paint_inputs(
    record: &GuiShapeRecord,
    position: [f32; 2],
    outer: f32,
) -> (u32, u32, [f32; 2], [f32; 2], [f32; 4], f32, f32) {
    let local = [
        position[0] - record.placement[0],
        position[1] - record.placement[1],
    ];
    let scale = [record.color1[2], record.color1[3]];
    let lane = (record.color1[0] + 0.5).floor();
    let slot = lane % GUI_PAINT_BLOCK_STRIDE;
    let block = (lane / GUI_PAINT_BLOCK_STRIDE).floor();
    (
        slot as u32,
        block as u32,
        [
            (local[0] - record.gradient_coords[0]) / scale[0],
            (local[1] - record.gradient_coords[1]) / scale[1],
        ],
        [record.gradient_coords[2], record.gradient_coords[3]],
        record.color0,
        outer / scale[0].abs().max(scale[1].abs()),
        record.color1[1],
    )
}

fn painted(
    style: &CanvasPrimitiveStyle,
    size: [f32; 2],
    shape: CanvasBoxShape,
    lanes: Option<GuiPaintLanes>,
) -> Vec<GuiShapeRecord> {
    painted_box_records(
        style,
        &size,
        &[0.0; 2],
        1.0,
        &[0.0, 0.0, 0.0, 1.0],
        &paint_fill(),
        None,
        &shape,
        CLIP,
        lanes,
    )
}

fn paint_fill() -> CanvasShapeFill {
    CanvasShapeFill::Paint {
        color: [0.25, 0.5, 0.75, 0.8],
        paint: CanvasTarget {
            entity: ipp_core::EntityId::from_bits(1),
            component: ipp_core::ComponentValue::CANVAS_PAINT,
            incarnation: 1,
        },
    }
}

#[test]
fn a_paint_sees_the_fragment_in_its_own_units_with_its_colour_and_slot() {
    let lanes = GuiPaintLanes {
        slot: 3,
        block: 117,
    };
    // A box at (10, 20), mirrored horizontally, scaled by two and half opaque, under
    // a tint that the paint's colour takes and its opacity does not.
    let mut style = style([10.0, 20.0], [-2.0, 2.0]);
    style.color = [0.5, 1.0, 1.0, 1.0];
    style.opacity = 0.5;
    let records = painted(&style, [40.0, 10.0], CanvasBoxShape::RECT, Some(lanes));
    let record = records[0];
    assert_eq!(record.material_params[0], GUI_FILL_PAINT);
    // The box's own top-left corner is at its placement; its own right edge lies to
    // the left on the Surface.
    let (slot, block, position, size, color, edge, opacity) =
        shader_paint_inputs(&record, [10.0 - 2.0 * 30.0, 20.0 + 2.0 * 5.0], -4.0);
    assert_eq!((slot, block), (3, 117));
    assert_eq!(position, [30.0, 5.0]);
    assert_eq!(size, [40.0, 10.0]);
    assert_eq!(color, [0.125, 0.5, 0.75, 0.8]);
    assert_eq!(edge, -2.0);
    assert_eq!(opacity, 0.5);

    // A stroke's placement is its own bounds; the part rectangle keeps the origin.
    let stroke = CanvasBoxShape::Stroke {
        segments: [[0.5, 0.0, 0.5, 1.0], [0.0; 4]],
    };
    let style = self::style([10.0, 20.0], [1.0, 1.0]);
    let record = painted(&style, [40.0, 10.0], stroke, Some(lanes))[0];
    assert_eq!(record.material_params[0], GUI_FILL_PAINT + GUI_PAINT_STROKE);
    assert_ne!(record.placement[0], 10.0);
    let (_, _, position, size, ..) = shader_paint_inputs(&record, [30.0, 25.0], 0.0);
    assert_eq!(position, [20.0, 5.0]);
    assert_eq!(size, [40.0, 10.0]);
}

#[test]
fn a_paint_without_lanes_draws_its_colour_and_lanes_change_the_hash_but_values_cannot() {
    let style = style([0.0, 0.0], [1.0, 1.0]);
    let solid = generate_box_records(
        &style,
        &[40.0, 10.0],
        &[0.0; 2],
        1.0,
        &[0.0, 0.0, 0.0, 1.0],
        &CanvasShapeFill::Solid([0.25, 0.5, 0.75, 0.8]),
        None,
        &CanvasBoxShape::RECT,
        CLIP,
    );
    assert_eq!(
        painted(&style, [40.0, 10.0], CanvasBoxShape::RECT, None),
        solid
    );

    let hash = |lanes| {
        hash_painted_box_inputs(
            &style,
            &[40.0, 10.0],
            &[0.0; 2],
            1.0,
            &[0.0, 0.0, 0.0, 1.0],
            &paint_fill(),
            None,
            &CanvasBoxShape::RECT,
            CLIP,
            lanes,
        )
    };
    let first = GuiPaintLanes {
        slot: 1,
        block: 0,
    };
    let moved = GuiPaintLanes {
        slot: 1,
        block: 4,
    };
    assert_ne!(hash(Some(first)), hash(Some(moved)));
    assert_ne!(hash(Some(first)), hash(None));
    assert_eq!(hash(Some(first)), hash(Some(first)));
}
