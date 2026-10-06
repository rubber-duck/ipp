//! Tests for arc geometry, its record lanes and the arc distance and coverage of
//! `surface_gui.frag`.
//!
//! [`shader_arc`] transliterates the shader's `arc_coverage` line for line and reads
//! only the generated lanes, so the brute-force and coverage tests check the packing
//! and the shader's formulas together against expectations derived independently from
//! the authored arc.

use super::super::records::GuiRecord;
use super::{
    GUI_BOX_ANTIALIAS_PAD, GUI_PAINT_ARC, GUI_PAINT_STROKE, GuiShapeRecord, generate_box_records,
    hash_box_inputs,
};
use ipp_core::systems::canvas::{
    CanvasBoxShape, CanvasClip, CanvasPart, CanvasPrimitiveId, CanvasPrimitiveStyle,
    CanvasShapeFill, CanvasShapeGlow, CanvasTarget,
};

const CLIP: CanvasClip = [-1000.0, -1000.0, 1000.0, 1000.0];

const SOLID: CanvasShapeFill = CanvasShapeFill::Solid([0.0, 0.9, 1.0, 1.0]);

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

fn arc(start: f32, sweep: f32, dashes: f32, dash_duty: f32) -> CanvasBoxShape {
    CanvasBoxShape::Arc {
        start,
        sweep,
        dashes,
        dash_duty,
    }
}

fn glow(radius: f32) -> CanvasShapeGlow {
    CanvasShapeGlow {
        color: [0.0, 1.0, 1.0, 1.0],
        intensity: 1.0,
        radius,
        inner_radius: 0.0,
        falloff: 1.0,
    }
}

/// One authored arc part: where and how large its rectangle is placed, the ring's
/// thickness, its optional glow and its shape.
#[derive(Clone, Copy)]
struct ArcPart {
    position: [f32; 2],
    scale: [f32; 2],
    size: [f32; 2],
    thickness: f32,
    glow: Option<f32>,
    shape: CanvasBoxShape,
}

impl ArcPart {
    fn new(size: f32, thickness: f32, shape: CanvasBoxShape) -> Self {
        Self {
            position: [0.0; 2],
            scale: [1.0; 2],
            size: [size; 2],
            thickness,
            glow: None,
            shape,
        }
    }

    fn records(&self) -> Vec<GuiShapeRecord> {
        generate_box_records(
            &style(self.position, self.scale),
            &self.size,
            &[0.0; 2],
            self.thickness,
            &[0.0; 4],
            &SOLID,
            self.glow.map(glow).as_ref(),
            &self.shape,
            CLIP,
        )
    }

    /// Placed centre and outer radius of the ring, from the authored rectangle.
    fn circle(&self) -> ([f64; 2], f64) {
        let placed = [
            f64::from(self.size[0] * self.scale[0]),
            f64::from(self.size[1] * self.scale[1]),
        ];
        let center = [
            f64::from(self.position[0]) + placed[0] / 2.0,
            f64::from(self.position[1]) + placed[1] / 2.0,
        ];
        (center, placed[0].abs().min(placed[1].abs()) / 2.0)
    }

    /// Placed direction of a clock angle in turns: clockwise from twelve o'clock in
    /// the box's own Y-down orientation, mirrored with the scale.
    fn direction(&self, turns: f64) -> [f64; 2] {
        let angle = turns * std::f64::consts::TAU;
        [
            angle.sin() * f64::from(self.scale[0].signum()),
            -angle.cos() * f64::from(self.scale[1].signum()),
        ]
    }

    /// Solid intervals of the arc in turns from its start along its sweep: the
    /// whole sweep, or each dash clipped to it.
    fn intervals(&self) -> Vec<[f64; 2]> {
        let CanvasBoxShape::Arc {
            sweep,
            dashes,
            dash_duty,
            ..
        } = self.shape
        else {
            unreachable!("arc part")
        };
        let extent = f64::from(sweep.abs().min(1.0));
        if dashes <= 0.0 || dash_duty >= 1.0 {
            return vec![[0.0, extent]];
        }
        let cell = 1.0 / f64::from(dashes);
        let duty = f64::from(dash_duty) * cell;
        (0..)
            .map(|index| (f64::from(index) + 0.5) * cell - duty / 2.0)
            .take_while(|start| *start < extent)
            .map(|start| [start, (start + duty).min(extent)])
            .collect()
    }

    /// Clock angle of a position along the sweep, in turns.
    fn along(&self, turns: f64) -> f64 {
        let CanvasBoxShape::Arc {
            start,
            sweep,
            ..
        } = self.shape
        else {
            unreachable!("arc part")
        };
        f64::from(start) + turns * f64::from(sweep.signum())
    }

    /// Whether a placed point lies in the painted ring sector, from the authored
    /// parameters alone.
    fn contains(&self, point: [f64; 2]) -> bool {
        let CanvasBoxShape::Arc {
            start,
            sweep,
            ..
        } = self.shape
        else {
            unreachable!("arc part")
        };
        let (center, outer) = self.circle();
        let inner = outer - f64::from(self.thickness).min(outer);
        // Back into the box's own orientation, where the clock angle is defined.
        let offset = [
            (point[0] - center[0]) * f64::from(self.scale[0].signum()),
            (point[1] - center[1]) * f64::from(self.scale[1].signum()),
        ];
        let radius = offset[0].hypot(offset[1]);
        if radius < inner || radius > outer {
            return false;
        }
        let clock = offset[0].atan2(-offset[1]) / std::f64::consts::TAU;
        let turns = ((clock - f64::from(start)) * f64::from(sweep.signum())).rem_euclid(1.0);
        self.intervals()
            .iter()
            .any(|[from, to]| (*from..=*to).contains(&turns))
    }

    /// Brute-force signed distance: the nearest of dense samples along every
    /// interval's two circles and two radial ends, negative inside.
    fn reference_distance(&self, samples: &[[f64; 2]], point: [f64; 2]) -> f64 {
        let nearest = samples
            .iter()
            .map(|sample| (sample[0] - point[0]).hypot(sample[1] - point[1]))
            .fold(f64::INFINITY, f64::min);
        if self.contains(point) {
            -nearest
        } else {
            nearest
        }
    }

    /// Boundary samples at most `step` apart.
    fn boundary(&self, step: f64) -> Vec<[f64; 2]> {
        let (center, outer) = self.circle();
        let inner = outer - f64::from(self.thickness).min(outer);
        let at = |turns: f64, radius: f64| {
            let direction = self.direction(self.along(turns));
            [
                center[0] + direction[0] * radius,
                center[1] + direction[1] * radius,
            ]
        };
        let mut samples = Vec::new();
        for [from, to] in self.intervals() {
            let count = ((to - from) * std::f64::consts::TAU * outer / step).ceil() as usize + 1;
            for index in 0..=count {
                let turns = from + (to - from) * index as f64 / count as f64;
                samples.push(at(turns, inner));
                samples.push(at(turns, outer));
            }
            if to - from < 1.0 {
                let count = ((outer - inner) / step).ceil() as usize + 1;
                for index in 0..=count {
                    let radius = inner + (outer - inner) * index as f64 / count as f64;
                    samples.push(at(from, radius));
                    samples.push(at(to, radius));
                }
            }
        }
        samples
    }
}

/// `arc_coverage` of `surface_gui.frag`, transliterated line for line: the signed
/// distance and coverage of the fragment at Surface `point`, whose Surface position
/// changes by `dx` and `dy` per pixel.
fn shader_arc(record: &GuiShapeRecord, point: [f32; 2], dx: [f32; 2], dy: [f32; 2]) -> (f32, f32) {
    // The shader's literals round to these values.
    use std::f32::consts::{PI, TAU};

    let dot = |a: [f32; 2], b: [f32; 2]| a[0] * b[0] + a[1] * b[1];
    let length = |a: [f32; 2]| (a[0] * a[0] + a[1] * a[1]).sqrt();
    let sign = |x: f32| {
        if x > 0.0 {
            1.0
        } else if x < 0.0 {
            -1.0
        } else {
            0.0
        }
    };
    let fract = |x: f32| x - x.floor();
    let band_coverage = |x: f32, half_extent: f32, footprint: f32| {
        ((half_extent - x) / footprint + 0.5).clamp(0.0, 1.0)
            - ((-half_extent - x) / footprint + 0.5).clamp(0.0, 1.0)
    };
    let arc_end_distance = |q: [f32; 2], radius: f32, half_width: f32| {
        sign(q[0]) * length([q[0], ((q[1] - radius).abs() - half_width).max(0.0)])
    };
    let dash_integral = |x: f32, duty: f32| x.floor() * duty + fract(x).min(duty);
    let smoothstep = |from: f32, to: f32, x: f32| {
        let t = ((x - from) / (to - from)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };

    let local = [
        point[0] - record.placement[0] - record.corner_cut[0],
        point[1] - record.placement[1] - record.corner_cut[1],
    ];
    let radius = record.shape[0];
    let half_sweep = record.shape[1];
    let half_width = record.shape[2].max(0.0) * 0.5;
    let middle = [record.corner_cut[2], record.corner_cut[3]];
    let cap = [record.corner_accent[0], record.corner_accent[1]];
    let cells = record.corner_accent[2];
    let turn = if cells < 0.0 {
        -1.0
    } else {
        1.0
    };
    let along = [-middle[1] * turn, middle[0] * turn];
    let p = [dot(local, along), dot(local, middle)];
    let r = length(p);
    let radial = if r > 0.0 {
        [local[0] / r, local[1] / r]
    } else {
        middle
    };
    let tangent = [-radial[1], radial[0]];
    let across = length([dot(dx, radial), dot(dy, radial)]).max(1.0 / 65536.0);
    let around = length([dot(dx, tangent), dot(dy, tangent)]).max(1.0 / 65536.0);
    let band = (r - radius).abs() - half_width;
    let ring = band_coverage(r - radius, half_width, across);
    let full = half_sweep > PI - 1.0e-5;

    if cells != 0.0 {
        let count = cells.abs();
        let duty = record.shape[3];
        let gap = (1.0 - duty) * 0.5;
        let span = half_sweep * count / PI;
        let last = ((span - gap).ceil() - 1.0).max(0.0);
        let last_end = (last + gap + duty).min(span);
        let phi = if r > 0.0 {
            p[0].atan2(p[1])
        } else {
            0.0
        };
        let u = (phi + half_sweep) * count / TAU;
        let width = (around * count / (TAU * r.max(1.0 / 65536.0))).max(1.0 / 65536.0);
        let lo = (if full {
            u - 0.5 * width
        } else {
            (u - 0.5 * width).clamp(0.0, span)
        }) - gap;
        let hi = (if full {
            u + 0.5 * width
        } else {
            (u + 0.5 * width).clamp(0.0, span)
        }) - gap;
        let base = lo.floor();
        let dashed = (dash_integral(hi - base, duty) - dash_integral(lo - base, duty)) / width;
        let fade = smoothstep(0.5, 1.0, width);
        let dashed = dashed + (duty * (hi - lo) / width - dashed) * fade;
        let coverage = ring * dashed.clamp(0.0, 1.0);

        let seam = gap - 0.5 * (count - (last_end - gap));
        let cell = u - count * ((u - seam) / count).floor();
        let first = cell.floor().clamp(0.0, last) + gap;
        let end = (first + duty).min(span);
        let mut offset = phi - ((first + end) * PI / count - half_sweep);
        offset -= TAU * (offset / TAU + 0.5).floor();
        let beyond = offset.abs() - (end - first) * PI / count;
        let outer = band.max(arc_end_distance(
            [r * beyond.sin(), r * beyond.cos()],
            radius,
            half_width,
        ));
        return (outer, coverage);
    }

    if full {
        return (band, ring);
    }
    let folded = [p[0].abs(), p[1]];
    let end = arc_end_distance(
        [
            folded[0] * cap[1] - folded[1] * cap[0],
            folded[0] * cap[0] + folded[1] * cap[1],
        ],
        radius,
        half_width,
    );
    let mut angular = (0.5 - end / around).clamp(0.0, 1.0);
    if half_sweep < 0.25 * PI || half_sweep > 0.75 * PI {
        let other = arc_end_distance(
            [
                -folded[0] * cap[1] - folded[1] * cap[0],
                folded[1] * cap[1] - folded[0] * cap[0],
            ],
            radius,
            half_width,
        );
        angular += (0.5 - other / around).clamp(0.0, 1.0)
            - if half_sweep < 0.25 * PI {
                1.0
            } else {
                0.0
            };
    }
    (band.max(end), ring * angular.clamp(0.0, 1.0))
}

/// Axis-aligned `[min_x, min_y, max_x, max_y]` of each generated quad.
fn quads(records: &[GuiShapeRecord]) -> Vec<[f32; 4]> {
    records.iter().map(|record| record.rect).collect()
}

/// Number of generated quads containing `point`.
fn covering(records: &[GuiShapeRecord], point: [f32; 2]) -> usize {
    quads(records)
        .iter()
        .filter(|quad| {
            point[0] >= quad[0] && point[0] <= quad[2] && point[1] >= quad[1] && point[1] <= quad[3]
        })
        .count()
}

fn assert_near<const N: usize>(actual: [f32; N], expected: [f32; N], tolerance: f32) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() <= tolerance),
        "{actual:?} != {expected:?}"
    );
}

const SQRT_HALF: f32 = std::f32::consts::FRAC_1_SQRT_2;

#[test]
fn an_arc_covers_its_sector_bounds_grown_by_its_glow_around_a_hollow_centre() {
    // A knob's 270-degree track from half past seven, clockwise over twelve o'clock
    // to half past four: a 40-unit ring 4 units thick, with a 3-unit glow.
    let knob = ArcPart {
        position: [10.0, 20.0],
        glow: Some(3.0),
        ..ArcPart::new(40.0, 4.0, arc(0.625, 0.75, 0.0, 1.0))
    };
    let records = knob.records();

    // The ends' outer corners bound it below; the outer circle's left, top and right
    // extremes lie within the sweep, its bottom does not.
    let bottom = 40.0 + 20.0 * SQRT_HALF;
    let placement = [10.0, 20.0, 40.0, bottom - 20.0];
    let pad = 3.0 + GUI_BOX_ANTIALIAS_PAD;
    for record in &records {
        assert_near(record.placement, placement, 1e-4);
        // Mean radius, half sweep, thickness and a solid duty.
        assert_near(
            record.shape,
            [18.0, 0.75 * std::f32::consts::PI, 4.0, 1.0],
            1e-6,
        );
        // The centre from the placement origin and the middle at twelve o'clock.
        assert_near(record.corner_cut, [20.0, 20.0, 0.0, -1.0], 1e-6);
        assert_near(
            record.corner_accent,
            [SQRT_HALF, -SQRT_HALF, 0.0, 0.0],
            1e-6,
        );
        assert_eq!(record.material_params, [GUI_PAINT_ARC, 0.0, 3.0, 1.0]);
    }

    // Four strips around the square inscribed in the disc nothing paints: the inner
    // radius less the glow and the antialias pad.
    let hole = (16.0 - pad) * SQRT_HALF;
    assert_eq!(records.len(), 4);
    let pieces = quads(&records);
    assert_near(
        pieces[0],
        [10.0 - pad, 20.0 - pad, 50.0 + pad, 40.0 - hole],
        1e-4,
    );
    assert_near(
        pieces[1],
        [10.0 - pad, 40.0 + hole, 50.0 + pad, bottom + pad],
        1e-4,
    );
    assert_near(
        pieces[2],
        [10.0 - pad, 40.0 - hole, 30.0 - hole, 40.0 + hole],
        1e-4,
    );
    for point in [
        // On the ring at nine, twelve and three o'clock and inside each end.
        [12.0, 40.0],
        [30.0, 22.0],
        [48.0, 40.0],
        [30.0 - 18.0 * SQRT_HALF + 0.5, 40.0 + 18.0 * SQRT_HALF - 0.5],
        // Glow beyond the half-past-four end's outer corner.
        [30.0 + 20.0 * SQRT_HALF + 1.5, 40.0 + 20.0 * SQRT_HALF + 1.5],
        // The glow reaching into the ring's centre.
        [30.0, 40.0 - 15.0],
    ] {
        assert_eq!(covering(&records, point), 1, "{point:?}");
    }
    assert_eq!(covering(&records, [30.0, 40.0]), 0);
    assert_eq!(covering(&records, [30.0, bottom + pad + 0.5]), 0);
}

#[test]
fn whole_and_partial_rings_fit_the_shorter_side_and_cover_only_what_paints() {
    // A whole ring in a wide rectangle fits its height, centred.
    let ring = ArcPart {
        size: [160.0, 120.0],
        ..ArcPart::new(0.0, 10.0, arc(0.3, 1.0, 0.0, 1.0))
    };
    let records = ring.records();
    assert_eq!(records.len(), 4);
    assert_near(records[0].placement, [20.0, 0.0, 120.0, 120.0], 1e-4);
    assert_eq!(records[0].shape[0], 55.0);
    assert_eq!(records[0].shape[1], std::f32::consts::PI);
    assert_eq!(records[0].corner_accent, [0.0, -1.0, 0.0, 0.0]);
    assert_eq!(covering(&records, [80.0, 60.0]), 0);
    assert_eq!(covering(&records, [80.0, 3.0]), 1);
    assert_eq!(covering(&records, [132.0, 60.0]), 1);

    // A sweep beyond a whole turn either way is the whole ring.
    for sweep in [1.5, -1.0, -7.0] {
        let other = ArcPart {
            shape: arc(0.3, sweep, 0.0, 1.0),
            ..ring
        };
        assert_eq!(other.records()[0].shape, records[0].shape);
    }

    // A thickness of the radius or more fills a pie: one quad over its sector.
    let pie = ArcPart::new(40.0, 25.0, arc(0.0, 0.25, 0.0, 1.0));
    let records = pie.records();
    assert_eq!(records.len(), 1);
    assert_near(
        records[0].shape,
        [10.0, 0.25 * std::f32::consts::PI, 20.0, 1.0],
        1e-6,
    );
    // The quarter from twelve to three o'clock: the centre is a corner.
    let pad = GUI_BOX_ANTIALIAS_PAD;
    assert_near(
        quads(&records)[0],
        [20.0 - pad, -pad, 40.0 + pad, 20.0 + pad],
        1e-5,
    );

    // A thin ring keeps the quad where the hole would not contain the bounds'
    // centre, as for a quarter arc.
    let quarter = ArcPart::new(40.0, 2.0, arc(0.0, 0.25, 0.0, 1.0));
    assert_eq!(quarter.records().len(), 1);
}

#[test]
fn arcs_mirror_with_their_box_and_wrap_at_one_turn() {
    let part = |start: f32, sweep: f32, dashes: f32, scale: [f32; 2]| {
        ArcPart {
            scale,
            ..ArcPart::new(40.0, 4.0, arc(start, sweep, dashes, 0.5))
        }
        .records()[0]
    };

    let middle = |record: GuiShapeRecord| [record.corner_cut[2], record.corner_cut[3]];

    // The start is taken modulo a turn, so a looping clip crossing the wrap moves
    // the arc continuously.
    let reference = part(0.9, 0.25, 8.0, [1.0, 1.0]);
    for start in [-0.1, 1.9, -5.1, 0.899_999_9] {
        let wrapped = part(start, 0.25, 8.0, [1.0, 1.0]);
        assert_near(wrapped.corner_cut, reference.corner_cut, 1e-4);
        assert_near(wrapped.placement, reference.placement, 1e-4);
    }
    assert_near(
        part(0.0, 0.5, 0.0, [1.0, 1.0]).corner_cut,
        part(1.0, 0.5, 0.0, [1.0, 1.0]).corner_cut,
        1e-6,
    );

    // Its middle lies a fortieth of a turn clockwise from twelve o'clock.
    let angle = 0.025 * std::f32::consts::TAU;
    assert_near(middle(reference), [angle.sin(), -angle.cos()], 1e-5);
    assert_eq!(reference.corner_accent[2], 8.0);

    // Mirroring the box mirrors the middle and reverses the sweep on the Surface,
    // as a negative sweep from the other end does.
    let mirrored = part(0.9, 0.25, 8.0, [-1.0, 1.0]);
    assert_near(middle(mirrored), [-angle.sin(), -angle.cos()], 1e-5);
    assert_eq!(mirrored.corner_accent[2], -8.0);
    let backwards = part(0.15, -0.25, 8.0, [1.0, 1.0]);
    assert_near(backwards.corner_cut, reference.corner_cut, 1e-4);
    assert_near(backwards.placement, reference.placement, 1e-4);
    assert_eq!(backwards.corner_accent[2], -8.0);
    // Both reverse the Surface direction twice.
    let flipped = part(0.9, -0.25, 8.0, [1.0, -1.0]);
    let angle = 0.775 * std::f32::consts::TAU;
    assert_near(middle(flipped), [angle.sin(), angle.cos()], 1e-5);
    assert_eq!(flipped.corner_accent[2], 8.0);
}

#[test]
fn arcs_that_paint_nothing_keep_degenerate_slots_and_dashes_normalize() {
    let nothing = |part: ArcPart| assert_eq!(part.records(), vec![GuiShapeRecord::EMPTY]);
    nothing(ArcPart::new(40.0, 4.0, arc(0.0, 0.0, 0.0, 1.0)));
    nothing(ArcPart::new(40.0, 4.0, arc(f32::NAN, 0.5, 0.0, 1.0)));
    nothing(ArcPart::new(40.0, 4.0, arc(0.0, f32::INFINITY, 0.0, 1.0)));
    nothing(ArcPart::new(0.0, 4.0, arc(0.0, 0.5, 0.0, 1.0)));
    // Dashes of no duty, and a sweep ending before its first dash, which starts half
    // a gap of a quarter cell after the start.
    nothing(ArcPart::new(40.0, 4.0, arc(0.0, 0.5, 8.0, 0.0)));
    nothing(ArcPart::new(40.0, 4.0, arc(0.0, 0.25 / 8.0, 8.0, 0.5)));
    assert_ne!(
        ArcPart::new(40.0, 4.0, arc(0.0, 0.26 / 8.0, 8.0, 0.5)).records(),
        vec![GuiShapeRecord::EMPTY]
    );

    // A whole duty, no cells or non-finite cells paint the solid arc.
    let solid = ArcPart::new(40.0, 4.0, arc(0.0, 0.5, 0.0, 1.0)).records();
    for (dashes, duty) in [(8.0, 1.0), (0.0, 0.3), (f32::INFINITY, 0.5)] {
        assert_eq!(
            ArcPart::new(40.0, 4.0, arc(0.0, 0.5, dashes, duty)).records(),
            solid
        );
    }
    let dashed = ArcPart::new(40.0, 4.0, arc(0.0, 0.5, 8.0, 0.25)).records();
    assert_eq!(dashed[0].shape[3], 0.25);
    assert_eq!(dashed[0].corner_accent[2], 8.0);
}

#[test]
fn arc_paint_follows_the_shape_stride_and_its_inputs_change_the_hash() {
    // A paint is the shape's offset plus the fill type.
    assert_eq!(GUI_PAINT_ARC, 2.0 * GUI_PAINT_STROKE);
    let gradient = CanvasShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [40.0, 0.0],
        start_color: [1.0; 4],
        end_color: [0.0, 0.0, 0.0, 1.0],
    };
    let records = generate_box_records(
        &style([0.0, 0.0], [1.0, 1.0]),
        &[40.0, 40.0],
        &[0.0; 2],
        4.0,
        &[0.0; 4],
        &gradient,
        None,
        &arc(0.25, 0.25, 0.0, 1.0),
        CLIP,
    );
    assert_eq!(records[0].material_params[0], 1.0 + GUI_PAINT_ARC);
    // The quarter from three to six o'clock: the gradient stays on the part.
    assert_near(records[0].placement, [20.0, 20.0, 20.0, 20.0], 1e-5);
    assert_eq!(records[0].gradient_coords, [-20.0, -20.0, 20.0, -20.0]);

    let hash = |shape: CanvasBoxShape| {
        hash_box_inputs(
            &style([0.0, 0.0], [1.0, 1.0]),
            &[40.0, 40.0],
            &[0.0; 2],
            4.0,
            &[0.0; 4],
            &SOLID,
            None,
            &shape,
            CLIP,
        )
    };
    let base = hash(arc(0.0, 0.5, 8.0, 0.5));
    for (index, variant) in [
        hash(arc(0.1, 0.5, 8.0, 0.5)),
        hash(arc(0.0, 0.6, 8.0, 0.5)),
        hash(arc(0.0, 0.5, 9.0, 0.5)),
        hash(arc(0.0, 0.5, 8.0, 0.6)),
        hash(CanvasBoxShape::RECT),
    ]
    .into_iter()
    .enumerate()
    {
        assert_ne!(variant, base, "variant {index}");
    }
}

/// Arcs whose shader distance the brute-force test checks: plain sweeps of every
/// width, a pie, mirrored and counter-clockwise arcs, whole dashed rings and partial
/// dashed arcs, one of them ending inside a dash.
fn distance_cases() -> Vec<ArcPart> {
    let part = |size, thickness, shape| ArcPart::new(size, thickness, shape);
    vec![
        part(40.0, 4.0, arc(0.625, 0.75, 0.0, 1.0)),
        part(40.0, 4.0, arc(0.1, 0.004, 0.0, 1.0)),
        part(40.0, 6.0, arc(0.2, 0.5, 0.0, 1.0)),
        part(40.0, 4.0, arc(0.0, 0.993, 0.0, 1.0)),
        part(40.0, 3.0, arc(0.4, 1.0, 0.0, 1.0)),
        part(40.0, 30.0, arc(0.3, 0.2, 0.0, 1.0)),
        ArcPart {
            scale: [-1.0, 1.0],
            ..part(40.0, 4.0, arc(0.05, 0.3, 0.0, 1.0))
        },
        part(40.0, 4.0, arc(0.8, -0.35, 0.0, 1.0)),
        part(40.0, 4.0, arc(0.0, 1.0, 8.0, 0.75)),
        part(40.0, 5.0, arc(0.6, 1.0, 12.0, 0.3)),
        part(40.0, 4.0, arc(0.625 - 1.0 / 96.0, 37.0 / 48.0, 48.0, 0.25)),
        part(40.0, 4.0, arc(0.1, 0.43, 8.0, 0.6)),
        ArcPart {
            scale: [1.0, -1.0],
            ..part(40.0, 4.0, arc(0.2, -0.61, 5.0, 0.5))
        },
    ]
}

#[test]
fn the_shader_arc_distance_matches_brute_force_reference_distances() {
    // Samples 0.02 apart bound the reference's error by 0.01; f32 adds little at a
    // 20-unit radius.
    const STEP: f64 = 0.02;
    const TOLERANCE: f64 = 0.02;
    let pixel = ([1.0, 0.0], [0.0, 1.0]);
    for (case, part) in distance_cases().into_iter().enumerate() {
        let record = part.records()[0];
        let samples = part.boundary(STEP);
        let (center, outer) = part.circle();
        let mut worst: (f64, [f64; 2]) = (0.0, [0.0; 2]);
        // A grid offset from the centre's axes, reaching beyond the ring by a glow.
        for row in 0..36 {
            for column in 0..36 {
                let point = [
                    center[0] - outer - 4.0
                        + (f64::from(column) + 0.37) * (2.0 * outer + 8.0) / 36.0,
                    center[1] - outer - 4.0 + (f64::from(row) + 0.61) * (2.0 * outer + 8.0) / 36.0,
                ];
                let (shader, _) = shader_arc(
                    &record,
                    [point[0] as f32, point[1] as f32],
                    pixel.0,
                    pixel.1,
                );
                let error = (f64::from(shader) - part.reference_distance(&samples, point)).abs();
                if error > worst.0 {
                    worst = (error, point);
                }
            }
        }
        assert!(
            worst.0 <= TOLERANCE,
            "case {case}: distance off by {} at {:?}",
            worst.0,
            worst.1
        );
    }
}

#[test]
fn the_shader_arc_coverage_matches_the_sector_with_a_fine_footprint() {
    let fine = ([1.0e-2, 0.0], [0.0, 1.0e-2]);
    for (case, part) in distance_cases().into_iter().enumerate() {
        let record = part.records()[0];
        let samples = part.boundary(0.02);
        let (center, outer) = part.circle();
        for row in 0..24 {
            for column in 0..24 {
                let point = [
                    center[0] - outer - 2.0
                        + (f64::from(column) + 0.43) * (2.0 * outer + 4.0) / 24.0,
                    center[1] - outer - 2.0 + (f64::from(row) + 0.29) * (2.0 * outer + 4.0) / 24.0,
                ];
                if part.reference_distance(&samples, point).abs() < 0.03 {
                    continue;
                }
                let (_, coverage) =
                    shader_arc(&record, [point[0] as f32, point[1] as f32], fine.0, fine.1);
                let expected = if part.contains(point) {
                    1.0
                } else {
                    0.0
                };
                assert!(
                    (coverage - expected).abs() < 1e-3,
                    "case {case}: coverage {coverage} at {point:?}, expected {expected}"
                );
            }
        }
    }
}

/// Coverage summed over `steps` positions `step` apart along `direction` from
/// `from`, times the step: the painted length a box filter sees along that line.
fn painted_length(
    record: &GuiShapeRecord,
    from: [f32; 2],
    direction: [f32; 2],
    step: f32,
    steps: usize,
    footprint: ([f32; 2], [f32; 2]),
) -> f32 {
    (0..steps)
        .map(|index| {
            let distance = step * index as f32;
            let point = [
                from[0] + direction[0] * distance,
                from[1] + direction[1] * distance,
            ];
            shader_arc(record, point, footprint.0, footprint.1).1 * step
        })
        .sum()
}

#[test]
fn the_shader_arc_coverage_keeps_the_area_of_details_finer_than_a_pixel() {
    let pixel = ([1.0, 0.0], [0.0, 1.0]);

    // A ring a quarter of a pixel thick keeps a quarter pixel across it, at three
    // o'clock where the line crosses it squarely.
    let thin = ArcPart::new(40.0, 0.25, arc(0.0, 1.0, 0.0, 1.0)).records()[0];
    let across = painted_length(&thin, [36.0, 20.0], [1.0, 0.0], 0.01, 800, pixel);
    assert!((across - 0.25).abs() < 0.01, "ring {across}");

    // A sweep thinner than a pixel, and a gap as thin, along the ring at its mean
    // radius: their ends share a pixel. Arc length 2 pi 18 / 500 = 0.226 units.
    let width = std::f32::consts::TAU * 18.0 / 500.0;
    let sliver = ArcPart::new(40.0, 4.0, arc(-0.001, 0.002, 0.0, 1.0)).records()[0];
    let along = painted_length(&sliver, [17.0, 2.0], [1.0, 0.0], 0.005, 1200, pixel);
    assert!((along - width).abs() < 0.01, "sliver {along} != {width}");
    let gap = ArcPart::new(40.0, 4.0, arc(0.001, 0.998, 0.0, 1.0)).records()[0];
    let cleared = 6.0 - painted_length(&gap, [17.0, 2.0], [1.0, 0.0], 0.005, 1200, pixel);
    assert!((cleared - width).abs() < 0.01, "gap {cleared} != {width}");

    // Minified dashes fade to their duty, without the beat a box filter of a
    // fractional number of cells leaves: footprints along the ring of 1.6 and 2
    // cells at the mean radius, and a fine one across it, at twelve o'clock.
    let cell = std::f32::consts::TAU * 18.0 / 48.0;
    let ticks = ArcPart::new(40.0, 4.0, arc(0.0, 1.0, 48.0, 0.25)).records()[0];
    for cells in [1.6, 2.0] {
        for offset in [0.0, 0.3, 1.1, -0.9] {
            let (_, coverage) = shader_arc(
                &ticks,
                [20.0 + offset, 2.0],
                [cells * cell, 0.0],
                [0.0, 1.0e-3],
            );
            assert!(
                (coverage - 0.25).abs() < 0.01,
                "minified {coverage} at {offset} over {cells} cells"
            );
        }
    }

    // A dash a third of a pixel wide keeps its width, to the first order of the
    // ring's curvature, when a line along the ring crosses it: the first dash,
    // centred 3.75 degrees past twelve o'clock.
    let fine = ArcPart::new(40.0, 4.0, arc(0.0, 1.0, 48.0, 0.3 / cell)).records()[0];
    let dash = painted_length(&fine, [20.3, 2.0], [1.0, 0.0], 0.005, 400, pixel);
    assert!((dash - 0.3).abs() < 0.015, "dash {dash}");
}
