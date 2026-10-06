//! Focused native GLES clip and GUI box frame oracle.
//!
//! Device-level counterpart to the raw Surface scenes in `egl_surfaces`: nested
//! clip intersections, scrolled curves/text-proxies/bitmaps, parameterized
//! rounded boxes drawn as instanced retained shape records with explicit per-axis
//! corner/border dimensions, empty-clip suppression, painter-order overlap, box
//! coverage ramps at close range and clip edges, premultiplied gradients, glow
//! falloff, cut corners, corner accents, inner glow, strokes, ring arcs with their
//! ends, dashes and glow, hue and saturation-value colour fields, a checker under an
//! alpha ramp, minified and on a tilted Surface, tilted, grazing
//! and perspective views, Surface cache targets with
//! nested atlas population and device replacement. All fixtures are synthetic
//! and local; assertions compare completed-frame pixels against values derived
//! independently from the scene layout and projection below.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod smoke;

#[cfg(target_os = "linux")]
use ipp_core::systems::canvas::{
    CanvasBoxShape, CanvasPart, CanvasPrimitiveId, CanvasPrimitiveStyle, CanvasShapeChecker,
    CanvasShapeFill, CanvasShapeGlow, CanvasTarget,
};

/// Surface content space: 4 x 3 metres at 80 pixels per metre on 320 x 240.
/// Content (x, y) in top-left/Y-down metres lands on pixel (80x, 80y).
#[cfg(target_os = "linux")]
const ROOT: [f32; 4] = [0.0, 0.0, 4.0, 3.0];

#[cfg(target_os = "linux")]
const MVP: [f32; 16] = [
    0.5, 0.0, 0.0, 0.0, 0.0, -0.6666667, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 1.0, 0.0, 1.0,
];

/// Close range: content (x, y) lands on pixel (2000x, 2000y), so the projected
/// antialias footprint is narrower than the generated geometry margin.
#[cfg(target_os = "linux")]
const CLOSE_MVP: [f32; 16] = [
    12.5,
    0.0,
    0.0,
    0.0,
    0.0,
    -16.666_666,
    0.0,
    0.0,
    0.0,
    0.0,
    1.0,
    0.0,
    -1.0,
    1.0,
    0.0,
    1.0,
];

/// Paints every rasterized fragment opaque magenta, exposing the production box
/// vertex shader's padded geometry independently of coverage.
#[cfg(target_os = "linux")]
const FOOTPRINT_FRAGMENT: &str = "#version 300 es
precision highp float;
out vec4 o_color;
void main() {
    o_color = vec4(1.0, 0.0, 1.0, 1.0);
}
";

/// One authored box: placement in Surface metres, straight linear colours and
/// explicit corner and border dimensions.
#[cfg(target_os = "linux")]
#[derive(Clone, Copy)]
struct ProbeBox {
    placement: [f32; 4],
    fill: [f32; 4],
    border_color: [f32; 4],
    corner: [f32; 2],
    border: f32,
}

#[cfg(target_os = "linux")]
impl ProbeBox {
    /// Production retained shape records for this box with any fill and glow.
    fn records_with(
        &self,
        fill: CanvasShapeFill,
        glow: Option<&CanvasShapeGlow>,
    ) -> Vec<ipp_render_gl::GuiShapeRecord> {
        self.records_shaped(fill, glow, CanvasBoxShape::RECT)
    }

    /// Production retained shape records for this box with any fill, glow and shape.
    fn records_shaped(
        &self,
        fill: CanvasShapeFill,
        glow: Option<&CanvasShapeGlow>,
        shape: CanvasBoxShape,
    ) -> Vec<ipp_render_gl::GuiShapeRecord> {
        let style = CanvasPrimitiveStyle {
            identity: CanvasPrimitiveId {
                target: CanvasTarget {
                    entity: ipp_core::EntityId::from_bits(0),
                    component: ipp_core::ComponentValue::CANVAS_BOX,
                    incarnation: 1,
                },
                part: CanvasPart::Content,
            },
            position: [self.placement[0], self.placement[1]],
            scale: [1.0, 1.0],
            color: [1.0; 4],
            opacity: 1.0,
            clip: ROOT,
            layer: 0,
        };
        ipp_render_gl::generate_gui_box_records(
            &style,
            &[self.placement[2], self.placement[3]],
            &self.corner,
            self.border,
            &self.border_color,
            &fill,
            glow,
            &shape,
            ROOT,
        )
    }

    fn records(&self) -> Vec<ipp_render_gl::GuiShapeRecord> {
        self.records_with(CanvasShapeFill::Solid(self.fill), None)
    }
}

/// Upload `records` into new retained GUI storage of their kind.
#[cfg(target_os = "linux")]
fn upload_records<D: ipp_render_gl::RenderDevice, R: ipp_render_gl::GuiRecord>(
    device: &mut D,
    records: &[R],
) -> Result<D::GuiBatch, Box<dyn std::error::Error>> {
    let mut batch = device.create_gui_batch(R::KIND, records.len())?;
    if let Err(error) = device.write_gui_batch(&mut batch, 0, records) {
        device.delete_gui_batch(batch);
        return Err(error.into());
    }
    Ok(batch)
}

/// Upload shape `records`, each clipped by `clip`, into new retained GUI storage.
#[cfg(target_os = "linux")]
fn upload<D: ipp_render_gl::RenderDevice>(
    device: &mut D,
    records: &[ipp_render_gl::GuiShapeRecord],
    clip: &[f32; 4],
) -> Result<D::GuiBatch, Box<dyn std::error::Error>> {
    let clipped: Vec<_> = records
        .iter()
        .map(|record| ipp_render_gl::GuiShapeRecord {
            clip: *clip,
            ..*record
        })
        .collect();
    upload_records(device, &clipped)
}

/// Draw shape `records` as one retained batch: allocate, draw once and release.
#[cfg(target_os = "linux")]
fn draw_batch<D: ipp_render_gl::RenderDevice>(
    device: &mut D,
    program: &D::Program,
    records: &[ipp_render_gl::GuiShapeRecord],
    mvp: &[f32; 16],
    clip: &[f32; 4],
) -> Result<(), Box<dyn std::error::Error>> {
    let batch = upload(device, records, clip)?;
    let drawn = device.draw_gui_batch(program, &batch, None, mvp, 0, records.len());
    device.delete_gui_batch(batch);
    Ok(drawn?)
}

/// Atlas glyph quad from `rect`'s first corner to its second, sampling `uv`'s
/// corners, clipped by `ROOT`.
#[cfg(target_os = "linux")]
fn glyph_record(rect: [f32; 4], uv: [f32; 4], color: [f32; 4]) -> ipp_render_gl::GuiGlyphRecord {
    ipp_render_gl::GuiGlyphRecord {
        rect,
        uv,
        color,
        clip: ROOT,
    }
}

/// The filled/rounded/bordered reference boxes, shared by the initial capture
/// and the device-replacement recovery check.
#[cfg(target_os = "linux")]
const REFERENCE_BOXES: [ProbeBox; 4] = [
    ProbeBox {
        placement: [0.5, 0.5, 1.0, 1.0],
        fill: [1.0, 0.0, 0.0, 1.0],
        border_color: [1.0; 4],
        corner: [0.0, 0.0],
        border: 0.0,
    },
    ProbeBox {
        placement: [2.0, 0.5, 1.0, 1.0],
        fill: [0.0, 1.0, 0.0, 1.0],
        border_color: [1.0; 4],
        corner: [0.35, 0.12],
        border: 0.0,
    },
    ProbeBox {
        placement: [0.5, 1.75, 1.0, 0.75],
        fill: [0.0, 0.0, 1.0, 1.0],
        border_color: [1.0; 4],
        corner: [0.1, 0.1],
        border: 0.05,
    },
    ProbeBox {
        placement: [2.0, 1.75, 1.0, 0.75],
        fill: [1.0, 1.0, 0.0, 1.0],
        border_color: [1.0; 4],
        corner: [0.0, 0.2],
        border: 0.0,
    },
];

#[cfg(target_os = "linux")]
fn draw_boxes<D: ipp_render_gl::RenderDevice>(
    device: &mut D,
    box_program: &D::Program,
) -> Result<(), Box<dyn std::error::Error>> {
    device.begin_frame(
        smoke::world::WIDTH,
        smoke::world::HEIGHT,
        &[0.0, 0.0, 0.0, 1.0],
    )?;
    for probe in REFERENCE_BOXES {
        draw_batch(device, box_program, &probe.records(), &MVP, &ROOT)?;
    }
    Ok(())
}

/// Display encoding applied once by the present pass, as 8-bit sRGB.
#[cfg(target_os = "linux")]
fn srgb(linear: f32) -> u8 {
    let linear = linear.clamp(0.0, 1.0);
    let encoded = if linear <= 0.003_130_8 {
        12.92 * linear
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (encoded * 255.0).round() as u8
}

/// Coverage of a pixel whose centre lies `distance` pixels outside a straight
/// contour (negative inside): a one-pixel linear ramp centred on the contour.
#[cfg(target_os = "linux")]
fn ramp(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

/// Column-major Surface-to-clip transforms and an independent projection oracle.
#[cfg(target_os = "linux")]
mod view {
    use super::smoke::world::{HEIGHT, WIDTH};

    /// Perspective camera at the origin looking down -Z, 60 degrees vertically.
    pub fn perspective() -> [f32; 16] {
        let (near, far) = (0.1_f32, 100.0_f32);
        let focal = 1.0 / 30.0_f32.to_radians().tan();
        let aspect = WIDTH as f32 / HEIGHT as f32;
        [
            focal / aspect,
            0.0,
            0.0,
            0.0,
            0.0,
            focal,
            0.0,
            0.0,
            0.0,
            0.0,
            (far + near) / (near - far),
            -1.0,
            0.0,
            0.0,
            2.0 * far * near / (near - far),
            0.0,
        ]
    }

    pub fn multiply(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
        std::array::from_fn(|index| {
            let (column, row) = (index / 4, index % 4);
            (0..4).map(|k| a[k * 4 + row] * b[column * 4 + k]).sum()
        })
    }

    pub fn rotation_x(angle: f32) -> [f32; 16] {
        let (sin, cos) = angle.sin_cos();
        [
            1.0, 0.0, 0.0, 0.0, 0.0, cos, sin, 0.0, 0.0, -sin, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    pub fn rotation_y(angle: f32) -> [f32; 16] {
        let (sin, cos) = angle.sin_cos();
        [
            cos, 0.0, -sin, 0.0, 0.0, 1.0, 0.0, 0.0, sin, 0.0, cos, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]
    }

    pub fn translation(x: f32, y: f32, z: f32) -> [f32; 16] {
        [
            1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, x, y, z, 1.0,
        ]
    }

    /// Place 4 x 3 metre Surface content (top-left origin, +Y down) on its centred
    /// local plane, orient it and view it through the perspective camera.
    pub fn surface(orientation: &[f32; 16], distance: f32) -> [f32; 16] {
        let content = [
            1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -2.0, 1.5, 0.0, 1.0,
        ];
        let model = multiply(
            &translation(0.0, 0.0, -distance),
            &multiply(orientation, &content),
        );
        multiply(&perspective(), &model)
    }

    /// Top-left pixel position and clip w of Surface content `point`.
    pub fn project(mvp: &[f32; 16], point: [f32; 2]) -> ([f32; 2], f32) {
        let clip: [f32; 4] = std::array::from_fn(|row| {
            mvp[row] * point[0] + mvp[4 + row] * point[1] + mvp[12 + row]
        });
        let w = clip[3];
        (
            [
                (clip[0] / w + 1.0) * 0.5 * WIDTH as f32,
                (1.0 - clip[1] / w) * 0.5 * HEIGHT as f32,
            ],
            w,
        )
    }

    /// Projected corners of a `[x, y, width, height]` content rectangle, in order.
    pub fn corners(mvp: &[f32; 16], rectangle: [f32; 4]) -> [[f32; 2]; 4] {
        let [x, y, width, height] = rectangle;
        [
            [x, y],
            [x + width, y],
            [x + width, y + height],
            [x, y + height],
        ]
        .map(|point| project(mvp, point).0)
    }

    /// `[min_x, max_x, min_y, max_y]` of projected points.
    pub fn bounds(points: &[[f32; 2]]) -> [f32; 4] {
        points.iter().fold(
            [f32::MAX, f32::MIN, f32::MAX, f32::MIN],
            |[min_x, max_x, min_y, max_y], [x, y]| {
                [min_x.min(*x), max_x.max(*x), min_y.min(*y), max_y.max(*y)]
            },
        )
    }

    /// Shoelace area of a projected polygon in square pixels.
    pub fn area(points: &[[f32; 2]]) -> f32 {
        let twice: f32 = (0..points.len())
            .map(|index| {
                let [a, b] = [points[index], points[(index + 1) % points.len()]];
                a[0] * b[1] - b[0] * a[1]
            })
            .sum();
        twice.abs() * 0.5
    }
}

/// Upload the unit-square contour through the production path packer.
#[cfg(target_os = "linux")]
fn unit_square(
    device: &mut ipp_render_gl::GlesRenderDevice,
) -> Result<
    (
        <ipp_render_gl::GlesRenderDevice as ipp_render_gl::RenderDevice>::SurfacePath,
        ipp_render_gl::SurfacePathDescriptor,
    ),
    Box<dyn std::error::Error>,
> {
    use ipp_core::services::asset_management::formats::quadratic::{
        QuadraticContour, QuadraticSegment,
    };
    use ipp_render_gl::RenderDevice;

    let line = |to| QuadraticSegment::Line {
        to,
    };
    let contours = [QuadraticContour {
        start: [0.0, 0.0],
        segments: vec![
            line([1.0, 0.0]),
            line([1.0, 1.0]),
            line([0.0, 1.0]),
            line([0.0, 0.0]),
        ],
    }];
    let atlas = ipp_render_gl::pack_surface_paths([([0.0, 0.0, 1.0, 1.0], contours.as_slice())]);
    Ok((
        device.create_surface_path(&atlas.texels)?,
        atlas.descriptors[0],
    ))
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use ipp_render_gl::RenderDevice;
    use std::path::PathBuf;

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err(
            "usage: egl_gui_clips <EGL-GLES-library-directory> <evidence-directory>".into(),
        );
    }
    let library_dir = PathBuf::from(&args[0]);
    let evidence = PathBuf::from(&args[1]);
    std::fs::create_dir_all(&evidence)?;

    const WIDTH: u32 = smoke::world::WIDTH;
    const HEIGHT: u32 = smoke::world::HEIGHT;
    const BLACK: [u8; 4] = [0, 0, 0, 255];

    let context = smoke::egl::Context::new(&library_dir, WIDTH, HEIGHT)?;
    let mut device = context.device()?;
    let path_program = device.create_program(
        include_str!("../src/services/render/shaders/surface.vert"),
        include_str!("../src/services/render/shaders/surface.frag"),
    )?;
    let instance_program = device.create_program(
        include_str!("../src/services/render/shaders/surface_instanced.vert"),
        include_str!("../src/services/render/shaders/surface.frag"),
    )?;
    let bitmap_program = device.create_program(
        include_str!("../src/services/render/shaders/surface_bitmap.vert"),
        include_str!("../src/services/render/shaders/surface_bitmap.frag"),
    )?;
    let box_program = device.create_program(
        include_str!("../src/services/render/shaders/surface_gui.vert"),
        include_str!("../src/services/render/shaders/surface_gui.frag"),
    )?;

    // Unit-square contour used as scrolled curves, text-proxy glyphs and
    // painter-order probes.
    let (square, square_descriptor) = unit_square(&mut device)?;
    // Two horizontal texels: left reddish, right greenish. Height one avoids
    // any row-order question; scroll assertions compare texels relatively.
    let stripes = device.create_texture(2, 1, &[255, 32, 32, 255, 32, 255, 32, 255])?;

    let pixel = |frame: &[u8], x: u32, y: u32| -> [u8; 4] {
        let offset = ((y * WIDTH + x) * 4) as usize;
        [
            frame[offset],
            frame[offset + 1],
            frame[offset + 2],
            frame[offset + 3],
        ]
    };
    let check = |frame: &[u8], x: u32, y: u32, expected: [u8; 4], tolerance: u8, label: &str| {
        let actual = pixel(frame, x, y);
        let close = actual
            .iter()
            .zip(expected)
            .all(|(a, e)| a.abs_diff(e) <= tolerance);
        if close {
            Ok(())
        } else {
            Err(format!(
                "{label}: pixel ({x},{y}) = {actual:?}, expected {expected:?}"
            ))
        }
    };
    let capture = |device: &mut ipp_render_gl::GlesRenderDevice, label: &str| {
        device.end_frame()?;
        let frame = context.capture()?;
        std::fs::write(evidence.join(format!("{label}.rgba")), &frame)?;
        Ok::<_, Box<dyn std::error::Error>>(frame)
    };

    // Nested clips: green nested inside red, sharing one placement.
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_path(
        &path_program,
        &square,
        &[0.0, 0.0, 1.0, 1.0],
        square_descriptor,
        &MVP,
        &[0.5, 0.5, 2.0, 2.0],
        &ROOT,
        &[1.0, 0.0, 0.0, 1.0],
        0,
    )?;
    device.draw_surface_path(
        &path_program,
        &square,
        &[0.0, 0.0, 1.0, 1.0],
        square_descriptor,
        &MVP,
        &[0.5, 0.5, 2.0, 2.0],
        &[1.0, 1.0, 2.0, 2.0],
        &[0.0, 1.0, 0.0, 1.0],
        0,
    )?;
    let nested = capture(&mut device, "clip-nested")?;
    check(
        &nested,
        120,
        120,
        [0, 255, 0, 255],
        8,
        "nested interior stays green",
    )?;
    check(
        &nested,
        60,
        60,
        [255, 0, 0, 255],
        8,
        "outer-only region stays red",
    )?;
    check(&nested, 240, 40, BLACK, 0, "outside path stays background")?;

    // Scrolled glyph proxies: two instanced squares under a tight clip.
    let glyphs = |x: f32| {
        [0.9 + x, 1.5 + x].map(|left| ipp_render_gl::SurfacePathInstance {
            bounds: [0.0, 0.0, 1.0, 1.0],
            placement: [left, 1.0, 0.2, 0.3],
            color: [0.0, 1.0, 0.0, 1.0],
            descriptor: square_descriptor,
        })
    };
    let text_clip = [1.0, 0.8, 2.0, 1.6];
    // One retained stream: created for the first frame, replaced in place when the
    // text scrolls.
    let mut stream = device.create_surface_instances(&square, &glyphs(0.0))?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_instances(&instance_program, &square, &stream, &MVP, &text_clip, 0)?;
    let text_before = capture(&mut device, "clip-text")?;
    device.update_surface_instances(&mut stream, &square, &glyphs(1.0))?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_instances(&instance_program, &square, &stream, &MVP, &text_clip, 0)?;
    let text_after = capture(&mut device, "clip-text-scrolled")?;
    device.delete_surface_instances(stream);
    check(
        &text_before,
        80,
        92,
        [0, 255, 0, 255],
        8,
        "first glyph visible before scroll",
    )?;
    check(
        &text_after,
        80,
        92,
        BLACK,
        0,
        "first glyph scrolled out of clip",
    )?;
    check(
        &text_before,
        128,
        92,
        [0, 255, 0, 255],
        8,
        "second glyph visible before scroll",
    )?;

    // Scrolled bitmap: shifting the placement moves texels under a fixed clip.
    let bitmap_clip = [1.0, 1.0, 3.0, 2.0];
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_bitmap(
        &bitmap_program,
        &stripes,
        &MVP,
        &[1.0, 1.0, 2.0, 1.0],
        &bitmap_clip,
        &[1.0; 4],
    )?;
    let bitmap_before = capture(&mut device, "clip-bitmap")?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_bitmap(
        &bitmap_program,
        &stripes,
        &MVP,
        &[0.0, 1.0, 2.0, 1.0],
        &bitmap_clip,
        &[1.0; 4],
    )?;
    let bitmap_after = capture(&mut device, "clip-bitmap-scrolled")?;
    if pixel(&bitmap_before, 120, 120) == pixel(&bitmap_before, 200, 120) {
        return Err("bitmap texels must differ across the stripe".into());
    }
    if pixel(&bitmap_after, 120, 120) != pixel(&bitmap_before, 200, 120) {
        return Err(format!(
            "scrolled bitmap must carry the right texel left: {:?} vs {:?}",
            pixel(&bitmap_after, 120, 120),
            pixel(&bitmap_before, 200, 120)
        )
        .into());
    }

    // Empty clips suppress draws without errors or pixels.
    let empty = [2.0, 2.0, 1.0, 1.0];
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_path(
        &path_program,
        &square,
        &[0.0, 0.0, 1.0, 1.0],
        square_descriptor,
        &MVP,
        &[0.5, 0.5, 1.0, 1.0],
        &empty,
        &[1.0, 0.0, 0.0, 1.0],
        0,
    )?;
    let suppressed_box = ProbeBox {
        placement: [0.5, 0.5, 1.0, 1.0],
        fill: [1.0, 0.0, 0.0, 1.0],
        border_color: [1.0; 4],
        corner: [0.1, 0.1],
        border: 0.02,
    };
    draw_batch(
        &mut device,
        &box_program,
        &suppressed_box.records(),
        &MVP,
        &empty,
    )?;
    let suppressed = capture(&mut device, "clip-empty")?;
    if suppressed
        .as_chunks::<4>()
        .0
        .iter()
        .any(|pixel| *pixel != BLACK)
    {
        return Err("empty clips must suppress every primitive".into());
    }

    // Filled, elliptically rounded, bordered and degenerate-radius boxes with
    // explicit dimensions. The unequal-radius corner samples distinguish the
    // shipped shader contour from scalar min-radius normalization.
    draw_boxes(&mut device, &box_program)?;
    let boxes = capture(&mut device, "boxes")?;
    check(&boxes, 80, 80, [255, 0, 0, 255], 8, "sharp filled interior")?;
    check(&boxes, 30, 30, BLACK, 0, "sharp exterior stays background")?;
    check(&boxes, 200, 80, [0, 255, 0, 255], 8, "rounded interior")?;
    check(
        &boxes,
        165,
        42,
        BLACK,
        8,
        "elliptical corner stays background",
    )?;
    check(
        &boxes,
        176,
        42,
        [0, 255, 0, 255],
        8,
        "elliptical top arc stays filled",
    )?;
    check(
        &boxes,
        200,
        41,
        [0, 255, 0, 255],
        8,
        "rounded edge stays filled",
    )?;
    check(
        &boxes,
        80,
        170,
        [0, 0, 255, 255],
        8,
        "bordered fill interior",
    )?;
    check(&boxes, 80, 142, [255, 255, 255, 255], 8, "border ring")?;
    check(
        &boxes,
        161,
        141,
        [255, 255, 0, 255],
        8,
        "zero-radius lane degrades to a sharp corner",
    )?;

    // Resizing keeps explicit corners and borders: corner blocks match exactly.
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    for placement in [[0.25, 0.25, 1.0, 1.0], [2.0, 0.25, 1.5, 2.0]] {
        let resized_box = ProbeBox {
            placement,
            fill: [1.0, 0.0, 0.0, 1.0],
            border_color: [1.0; 4],
            corner: [0.15, 0.15],
            border: 0.03,
        };
        draw_batch(
            &mut device,
            &box_program,
            &resized_box.records(),
            &MVP,
            &ROOT,
        )?;
    }
    let resized = capture(&mut device, "boxes-resized")?;
    let corner = |frame: &[u8], x: u32, y: u32| {
        let mut block = Vec::with_capacity(12 * 12 * 4);
        for row in y..y + 12 {
            for column in x..x + 12 {
                block.extend_from_slice(&pixel(frame, column, row));
            }
        }
        block
    };
    if corner(&resized, 20, 20) != corner(&resized, 160, 20) {
        return Err("resized boxes must keep identical corner pixels".into());
    }

    // Painter order: later boxes win the overlap in draw order. Each box keeps
    // its placement, color and shape; only the draw sequence reverses.
    let red = ProbeBox {
        placement: [1.0, 1.0, 1.5, 1.0],
        fill: [1.0, 0.0, 0.0, 1.0],
        border_color: [0.0, 0.0, 0.0, 1.0],
        corner: [0.0, 0.0],
        border: 0.0,
    };
    let green = ProbeBox {
        placement: [1.75, 1.25, 1.5, 1.0],
        fill: [0.0, 1.0, 0.0, 1.0],
        corner: [0.2, 0.2],
        ..red
    };
    let paint = |device: &mut ipp_render_gl::GlesRenderDevice, order: [ProbeBox; 2]| {
        device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
        for entry in order {
            draw_batch(device, &box_program, &entry.records(), &MVP, &ROOT)?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    paint(&mut device, [red, green])?;
    let green_over = capture(&mut device, "boxes-green-over-red")?;
    paint(&mut device, [green, red])?;
    let red_over = capture(&mut device, "boxes-red-over-green")?;
    check(
        &green_over,
        168,
        128,
        [0, 255, 0, 255],
        8,
        "later green wins overlap",
    )?;
    check(
        &red_over,
        168,
        128,
        [255, 0, 0, 255],
        8,
        "later red wins overlap",
    )?;

    // Tilted view: boxes keep coverage and clipping under a rotated MVP.
    let tilt = 0.5_f32;
    let (sin, cos) = tilt.sin_cos();
    let tilted = [
        MVP[0],
        MVP[1] * cos,
        MVP[1] * sin,
        0.0,
        MVP[4],
        MVP[5] * cos,
        MVP[5] * sin,
        0.0,
        0.0,
        -sin,
        cos,
        0.0,
        MVP[12],
        MVP[13],
        MVP[14],
        MVP[15],
    ];
    let tilted_box = ProbeBox {
        placement: [1.0, 0.75, 2.0, 1.5],
        fill: [0.0, 1.0, 0.0, 1.0],
        border_color: [1.0; 4],
        corner: [0.2, 0.2],
        border: 0.0,
    };
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &tilted_box.records(),
        &tilted,
        &ROOT,
    )?;
    let tilted_frame = capture(&mut device, "boxes-tilted")?;
    let green_count = tilted_frame
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[1] > 180 && pixel[0] < 90)
        .count();
    if green_count < 300 {
        return Err(format!("tilted box coverage too small: {green_count}").into());
    }
    check(
        &tilted_frame,
        316,
        236,
        BLACK,
        0,
        "tilted exterior stays background",
    )?;

    // Surface cache targets restore their repaint target around nested atlas pages.
    smoke::surface_cache_target::run(&context, &mut device, &evidence)?;

    // Device replacement redraws identical boxes after re-upload.
    device.delete_surface_path(square);
    device.delete_texture(stripes);
    device.delete_program(path_program);
    device.delete_program(instance_program);
    device.delete_program(bitmap_program);
    device.delete_program(box_program);
    drop(device);
    let mut device = context.device()?;
    let box_program = device.create_program(
        include_str!("../src/services/render/shaders/surface_gui.vert"),
        include_str!("../src/services/render/shaders/surface_gui.frag"),
    )?;
    draw_boxes(&mut device, &box_program)?;
    let recovered = capture(&mut device, "boxes-recovered")?;
    if recovered != boxes {
        return Err("recreated device must redraw identical boxes".into());
    }

    // GUI batch with gradients, borders, and localized glow.
    let linear_style = ipp_core::systems::canvas::CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: CanvasTarget {
                entity: ipp_core::EntityId::from_bits(10),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Content,
        },
        position: [0.5, 0.5],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: ROOT,
        layer: 0,
    };
    let linear_fill = ipp_core::systems::canvas::CanvasShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [1.0, 1.0],
        start_color: [1.0, 0.0, 0.0, 1.0],
        end_color: [0.0, 0.0, 1.0, 1.0],
    };
    let mut batch_records = Vec::new();
    batch_records.extend_from_slice(&ipp_render_gl::generate_gui_box_records(
        &linear_style,
        &[1.0, 1.0],
        &[0.1, 0.1],
        0.0,
        &[0.0; 4],
        &linear_fill,
        None,
        &CanvasBoxShape::RECT,
        ROOT,
    ));

    let radial_style = ipp_core::systems::canvas::CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: CanvasTarget {
                entity: ipp_core::EntityId::from_bits(11),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Content,
        },
        position: [2.0, 0.5],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: ROOT,
        layer: 0,
    };
    let radial_fill = ipp_core::systems::canvas::CanvasShapeFill::RadialGradient {
        center: [0.5, 0.5],
        radius: 0.5,
        start_color: [1.0, 1.0, 0.0, 1.0],
        end_color: [0.5, 0.0, 0.5, 1.0],
    };
    batch_records.extend_from_slice(&ipp_render_gl::generate_gui_box_records(
        &radial_style,
        &[1.0, 1.0],
        &[0.35, 0.12],
        0.0,
        &[0.0; 4],
        &radial_fill,
        None,
        &CanvasBoxShape::RECT,
        ROOT,
    ));

    let glow_style = ipp_core::systems::canvas::CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: CanvasTarget {
                entity: ipp_core::EntityId::from_bits(12),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Content,
        },
        position: [0.5, 1.75],
        scale: [1.0, 1.0],
        color: [1.0; 4],
        opacity: 1.0,
        clip: ROOT,
        layer: 0,
    };
    let glow = ipp_core::systems::canvas::CanvasShapeGlow {
        color: [0.0, 1.0, 0.0, 1.0],
        intensity: 1.0,
        radius: 0.15,
        inner_radius: 0.0,
        falloff: 1.5,
    };
    batch_records.extend_from_slice(&ipp_render_gl::generate_gui_box_records(
        &glow_style,
        &[1.0, 0.75],
        &[0.1, 0.1],
        0.0,
        &[0.0; 4],
        &ipp_core::systems::canvas::CanvasShapeFill::Solid([0.0, 1.0, 1.0, 1.0]),
        Some(&glow),
        &CanvasBoxShape::RECT,
        ROOT,
    ));

    let border_style = ipp_core::systems::canvas::CanvasPrimitiveStyle {
        identity: CanvasPrimitiveId {
            target: CanvasTarget {
                entity: ipp_core::EntityId::from_bits(13),
                component: ipp_core::ComponentValue::CANVAS_BOX,
                incarnation: 1,
            },
            part: CanvasPart::Content,
        },
        position: [2.0, 1.75],
        scale: [1.0, 1.0],
        color: [1.0; 4],
        opacity: 1.0,
        clip: ROOT,
        layer: 0,
    };
    let border_records = ipp_render_gl::generate_gui_box_records(
        &border_style,
        &[1.0, 0.75],
        &[0.1, 0.1],
        0.1,
        &[1.0, 0.5, 0.0, 1.0],
        &ipp_core::systems::canvas::CanvasShapeFill::Solid([0.0; 4]),
        None,
        &CanvasBoxShape::RECT,
        ROOT,
    );
    assert_eq!(
        border_records.len(),
        4,
        "border-only must use 4 edge strips"
    );
    batch_records.extend_from_slice(&border_records);

    // A small glowing outline uses a full quad, so the shader must keep its
    // hollow center clear independently of the sparse-outline optimization.
    let hollow_style = ipp_core::systems::canvas::CanvasPrimitiveStyle {
        position: [3.4, 1.75],
        ..border_style
    };
    let hollow_records = ipp_render_gl::generate_gui_box_records(
        &hollow_style,
        &[0.4, 0.4],
        &[0.05, 0.05],
        0.02,
        &[1.0, 0.5, 0.0, 1.0],
        &ipp_core::systems::canvas::CanvasShapeFill::Solid([0.0; 4]),
        Some(&glow),
        &CanvasBoxShape::RECT,
        ROOT,
    );
    assert_eq!(hollow_records.len(), 1);
    batch_records.extend_from_slice(&hollow_records);

    let batch = upload(&mut device, &batch_records, &ROOT)?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_gui_batch(&box_program, &batch, None, &MVP, 0, batch_records.len())?;
    let materials_frame = capture(&mut device, "boxes-materials")?;
    check(
        &materials_frame,
        80,
        80,
        [187, 0, 187, 255],
        8,
        "linear gradient center",
    )?;
    check(
        &materials_frame,
        200,
        80,
        [255, 255, 0, 255],
        24,
        "radial gradient center",
    )?;
    check(
        &materials_frame,
        80,
        170,
        [0, 255, 255, 255],
        16,
        "glow box interior",
    )?;
    check(
        &materials_frame,
        200,
        170,
        BLACK,
        0,
        "border-only hollow interior",
    )?;
    check(
        &materials_frame,
        200,
        144,
        [255, 188, 0, 255],
        8,
        "border-only edge strip",
    )?;
    check(
        &materials_frame,
        288,
        156,
        BLACK,
        0,
        "outer glow preserves the hollow outline center",
    )?;
    let halo_pixel = pixel(&materials_frame, 264, 156);
    assert!(halo_pixel[1] > 30 && halo_pixel[0] < 10 && halo_pixel[2] < 10);
    device.delete_gui_batch(batch);
    // A one-pixel rail and one-pixel hollow border must retain their contrast.
    // The halo belongs outside that crisp silhouette, with no tinted center or
    // extra blur of the opaque rail. Pixel-aligned probes are independent of
    // the shader's distance/coverage implementation.
    let rail_style = ipp_core::systems::canvas::CanvasPrimitiveStyle {
        position: [0.5, 0.5],
        ..glow_style
    };
    let rail = ipp_render_gl::generate_gui_box_records(
        &rail_style,
        &[1.0, 0.0125],
        &[0.0, 0.0],
        0.0,
        &[0.0; 4],
        &ipp_core::systems::canvas::CanvasShapeFill::Solid([1.0; 4]),
        Some(&glow),
        &CanvasBoxShape::RECT,
        ROOT,
    );
    let outline_style = ipp_core::systems::canvas::CanvasPrimitiveStyle {
        position: [2.0, 0.5],
        ..rail_style
    };
    let outline = ipp_render_gl::generate_gui_box_records(
        &outline_style,
        &[1.0, 0.5],
        &[0.0, 0.0],
        0.0125,
        &[1.0; 4],
        &ipp_core::systems::canvas::CanvasShapeFill::Solid([0.0; 4]),
        Some(&glow),
        &CanvasBoxShape::RECT,
        ROOT,
    );
    let sharp_records = [rail, outline].concat();
    let sharp_batch = upload(&mut device, &sharp_records, &ROOT)?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_gui_batch(
        &box_program,
        &sharp_batch,
        None,
        &MVP,
        0,
        sharp_records.len(),
    )?;
    let sharp = capture(&mut device, "boxes-sharp-edges-and-glow")?;
    check(&sharp, 80, 40, [255; 4], 4, "one-pixel rail stays opaque")?;
    check(
        &sharp,
        200,
        40,
        [255; 4],
        4,
        "one-pixel border stays opaque",
    )?;
    check(
        &sharp,
        200,
        41,
        BLACK,
        4,
        "border stops at its inner contour",
    )?;
    check(&sharp, 200, 60, BLACK, 0, "halo leaves hollow center clear")?;
    let mut previous = 255;
    for y in (27..40).rev() {
        let halo = pixel(&sharp, 80, y);
        assert!(
            halo[0] <= 4 && halo[2] <= 4,
            "rail fill leaked into halo: {halo:?}"
        );
        assert!(halo[1] <= previous, "halo must fade away from the rail");
        previous = halo[1];
    }
    check(&sharp, 80, 27, BLACK, 0, "halo has bounded extent")?;
    device.delete_gui_batch(sharp_batch);

    // Close range: the projected footprint (0.75 mm) is narrower than the generated
    // 2 mm margin. A hollow border drawn as sparse strips still antialiases its inner
    // contour with the same one-pixel ramp as its outer contour, here at 40.25 and
    // 48.25 pixels on both axes.
    let hollow = ProbeBox {
        placement: [0.020_125, 0.020_125, 0.12, 0.08],
        fill: [0.0; 4],
        border_color: [1.0; 4],
        corner: [0.0, 0.0],
        border: 0.004,
    };
    let hollow_records = hollow.records();
    assert_eq!(
        hollow_records.len(),
        4,
        "hollow border must use sparse strips"
    );

    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &hollow_records,
        &CLOSE_MVP,
        &ROOT,
    )?;
    let close = capture(&mut device, "boxes-close-range-border")?;

    let mut close_profile = Vec::new();
    for offset in 38..51_u32 {
        let centre = offset as f32 + 0.5;
        let shape = ramp(40.25 - centre);
        let level = srgb(shape - shape.min(ramp(48.25 - centre)));
        let expected = [level, level, level, 255];
        check(
            &close,
            offset,
            120,
            expected,
            6,
            "close-range left border ramp",
        )?;
        check(
            &close,
            160,
            offset,
            expected,
            6,
            "close-range top border ramp",
        )?;
        close_profile.push(pixel(&close, offset, 120)[0]);
    }
    check(
        &close,
        160,
        120,
        BLACK,
        0,
        "close-range hollow centre stays clear",
    )?;

    // Clip edges antialias over one pixel in Surface coordinates: the left clip edge
    // passes through the centre of column 120 and the right one lies a quarter pixel
    // beyond the centre of column 199.
    let clipped_box = ProbeBox {
        placement: [1.0, 1.0, 2.0, 1.0],
        fill: [1.0; 4],
        border_color: [1.0; 4],
        corner: [0.0, 0.0],
        border: 0.0,
    };
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &clipped_box.records(),
        &MVP,
        &[120.5 / 80.0, 0.0, 200.25 / 80.0, 3.0],
    )?;
    let clip_edge = capture(&mut device, "boxes-clip-edge")?;

    for (column, coverage) in [
        (119, 0.0),
        (120, 0.5),
        (121, 1.0),
        (199, 1.0),
        (200, 0.25),
        (201, 0.0),
    ] {
        let level = srgb(coverage);
        check(
            &clip_edge,
            column,
            120,
            [level, level, level, 255],
            6,
            "clip edge ramp",
        )?;
    }

    // Gradient stops with different alpha interpolate premultiplied: fading opaque red
    // into fully transparent blue leaves no blue fringe over the black background.
    let gradient_box = ProbeBox {
        placement: [0.5, 0.5, 2.0, 1.0],
        fill: [0.0; 4],
        border_color: [0.0; 4],
        corner: [0.0, 0.0],
        border: 0.0,
    };
    let fading = CanvasShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [2.0, 0.0],
        start_color: [1.0, 0.0, 0.0, 1.0],
        end_color: [0.0, 0.0, 1.0, 0.0],
    };
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &gradient_box.records_with(fading, None),
        &MVP,
        &ROOT,
    )?;
    let gradient = capture(&mut device, "boxes-gradient-alpha")?;

    for column in (44..196).step_by(8) {
        let t = ((column as f32 + 0.5) / 80.0 - 0.5) / 2.0;
        check(
            &gradient,
            column,
            80,
            [srgb(1.0 - t), 0, 0, 255],
            6,
            "premultiplied gradient without fringe",
        )?;
    }

    // Glow covers only the pixel area outside the shape and is attenuated once. The
    // opaque red box's right contour passes through the centre of column 160, so that
    // pixel is half fill and half full-strength glow; beyond it glow alpha follows the
    // authored quadratic falloff over its 0.25 m (20 pixel) radius.
    let glowing = ProbeBox {
        placement: [1.0, 1.0, 160.5 / 80.0 - 1.0, 1.0],
        fill: [1.0, 0.0, 0.0, 1.0],
        border_color: [0.0; 4],
        corner: [0.0, 0.0],
        border: 0.0,
    };
    let falloff_glow = CanvasShapeGlow {
        color: [0.0, 1.0, 0.0, 1.0],
        intensity: 1.0,
        radius: 0.25,
        inner_radius: 0.0,
        falloff: 2.0,
    };
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &glowing.records_with(CanvasShapeFill::Solid(glowing.fill), Some(&falloff_glow)),
        &MVP,
        &ROOT,
    )?;
    let glow_frame = capture(&mut device, "boxes-glow-falloff")?;

    check(
        &glow_frame,
        159,
        120,
        [255, 0, 0, 255],
        6,
        "glowing fill interior",
    )?;
    let half = srgb(0.5);
    check(
        &glow_frame,
        160,
        120,
        [half, half, 0, 255],
        6,
        "contour pixel splits fill and glow",
    )?;
    for distance in 1..=22_u32 {
        let norm = (1.0 - distance as f32 / 20.0).max(0.0);
        check(
            &glow_frame,
            160 + distance,
            120,
            [0, srgb(norm * norm), 0, 255],
            6,
            "single-attenuation glow falloff",
        )?;
    }

    // Per-corner shapes and strokes keep the box's one-pixel ramps. At 80 pixels per
    // metre, pixel (x, y) covers content [x, x + 1] / 80 m; distances below are in
    // pixels from pixel centres, positive outside a contour.
    let px = |pixels: f32| pixels / 80.0;
    let white = [1.0; 4];
    let grey = |coverage: f32| {
        let level = srgb(coverage);
        [level, level, level, 255]
    };
    let rect_shape = |cut: [f32; 4], accent: [f32; 4], accent_width: f32| CanvasBoxShape::Rect {
        corner_cut: cut.map(|length| length / 80.0),
        corner_accent: accent.map(|length| length / 80.0),
        corner_accent_width: accent_width / 80.0,
        checker: None,
    };
    let centre = |index: u32| index as f32 + 0.5;

    // A 20-pixel top-left cut of an 80-pixel box at (left, 80) runs along
    // x + y = left + 100 between (left + 20, 80) and (left, 100).
    let cut_outside = |left: f32, x: u32, y: u32| {
        (left + 100.0 - centre(x) - centre(y)) / std::f32::consts::SQRT_2
    };
    let cut = rect_shape([20.0, 0.0, 0.0, 0.0], [0.0; 4], 0.0);
    let cut_box = ProbeBox {
        placement: [px(80.0), px(80.0), px(80.0), px(80.0)],
        fill: white,
        border_color: white,
        corner: [0.0, 0.0],
        border: 0.0,
    };
    // A half-pixel border along the same contour, drawn as sparse corner squares and
    // edge strips because its fill is transparent.
    let ring_box = ProbeBox {
        placement: [px(180.0), px(80.0), px(80.0), px(80.0)],
        fill: [0.0; 4],
        border: px(0.5),
        ..cut_box
    };
    let ring_records = ring_box.records_shaped(CanvasShapeFill::Solid([0.0; 4]), None, cut);
    assert_eq!(
        ring_records.len(),
        8,
        "hollow cut box must use sparse corners and strips"
    );
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &cut_box.records_shaped(CanvasShapeFill::Solid(white), None, cut),
        &MVP,
        &ROOT,
    )?;
    draw_batch(&mut device, &box_program, &ring_records, &MVP, &ROOT)?;
    let cut_frame = capture(&mut device, "shapes-cut-corner")?;

    let ring = |outside: f32| ramp(outside) - ramp(outside).min(ramp(outside + 0.5));
    for column in 84..=93 {
        let outside = cut_outside(80.0, column, 90);
        check(
            &cut_frame,
            column,
            90,
            grey(ramp(outside)),
            6,
            "cut corner diagonal ramp",
        )?;
        let outside = cut_outside(180.0, column + 100, 90);
        check(
            &cut_frame,
            column + 100,
            90,
            grey(ring(outside)),
            6,
            "sub-pixel border ramp along a cut",
        )?;
    }
    for row in 79..=81 {
        check(
            &cut_frame,
            220,
            row,
            grey(ring(80.0 - centre(row))),
            6,
            "sub-pixel border ramp beside a cut",
        )?;
    }
    check(&cut_frame, 82, 82, BLACK, 0, "cut removes its corner")?;
    check(
        &cut_frame,
        220,
        120,
        BLACK,
        0,
        "hollow cut box centre stays clear",
    )?;

    // Glow beyond a cut follows the exact distance to the cut contour: past the cut's
    // end it rounds around the end point instead of continuing the mitred planes.
    let cut_glow = CanvasShapeGlow {
        color: [0.0, 1.0, 0.0, 1.0],
        intensity: 1.0,
        radius: px(20.0),
        inner_radius: 0.0,
        falloff: 1.0,
    };
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &cut_box.records_shaped(
            CanvasShapeFill::Solid([0.0, 0.0, 1.0, 1.0]),
            Some(&cut_glow),
            cut,
        ),
        &MVP,
        &ROOT,
    )?;
    let cut_glow_frame = capture(&mut device, "shapes-cut-glow")?;
    for (x, y, distance, label) in [
        (
            95,
            68,
            (centre(95) - 100.0).hypot(centre(68) - 80.0),
            "glow rounds the cut's end",
        ),
        (120, 68, 80.0 - centre(68), "glow beside the top edge"),
        (82, 82, cut_outside(80.0, 82, 82), "glow beyond the cut"),
    ] {
        check(
            &cut_glow_frame,
            x,
            y,
            [0, srgb(1.0 - distance / 20.0), 0, 255],
            4,
            label,
        )?;
    }

    // Inner glow falls inward from the outer contour beneath a 2-pixel border, over
    // a transparent fill, for 16 pixels.
    let inner_glow = CanvasShapeGlow {
        radius: 0.0,
        inner_radius: px(16.0),
        ..cut_glow
    };
    let framed = ProbeBox {
        placement: [px(80.0), px(80.0), px(80.0), px(80.0)],
        fill: [0.0; 4],
        border_color: white,
        corner: [0.0, 0.0],
        border: px(2.0),
    };
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &framed.records_with(CanvasShapeFill::Solid([0.0; 4]), Some(&inner_glow)),
        &MVP,
        &ROOT,
    )?;
    let inner_frame = capture(&mut device, "shapes-inner-glow")?;
    for column in 79..=100 {
        let depth = centre(column) - 80.0;
        let shape = ramp(-depth);
        let fill = shape.min(ramp(2.0 - depth));
        let glow = (1.0 - depth.max(0.0) / 16.0).clamp(0.0, 1.0);
        let border = srgb(shape - fill);
        check(
            &inner_frame,
            column,
            120,
            [border, srgb(shape - fill + glow * fill), border, 255],
            6,
            "inner glow beneath the border",
        )?;
    }
    check(
        &inner_frame,
        120,
        120,
        BLACK,
        0,
        "inner glow ends before the centre",
    )?;

    // Corner accents thicken a 1-pixel border to 4 pixels within 20.5 pixels of each
    // corner, ending mid-pixel; without the border they are brackets alone.
    let accents = rect_shape([0.0; 4], [20.5; 4], 4.0);
    let frame_box = ProbeBox {
        placement: [px(20.0), px(20.0), px(120.0), px(80.0)],
        fill: [0.0; 4],
        border_color: white,
        corner: [0.0, 0.0],
        border: px(1.0),
    };
    let brackets_box = ProbeBox {
        placement: [px(180.0), px(20.0), px(120.0), px(80.0)],
        border: 0.0,
        ..frame_box
    };
    let frame_records = frame_box.records_shaped(CanvasShapeFill::Solid([0.0; 4]), None, accents);
    let bracket_records =
        brackets_box.records_shaped(CanvasShapeFill::Solid([0.0; 4]), None, accents);
    assert_eq!(frame_records.len(), 8, "accented frame uses corner squares");
    assert_eq!(
        bracket_records.len(),
        4,
        "brackets alone cover only their corners"
    );
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(&mut device, &box_program, &frame_records, &MVP, &ROOT)?;
    draw_batch(&mut device, &box_program, &bracket_records, &MVP, &ROOT)?;
    let accent_frame = capture(&mut device, "shapes-corner-accents")?;
    let band = |width: f32, row: u32| {
        let depth = centre(row) - 20.0;
        ramp(-depth) - ramp(-depth).min(ramp(width - depth))
    };
    for (left, border) in [(20, 1.0), (180, 0.0)] {
        for row in 19..=25 {
            check(
                &accent_frame,
                left + 10,
                row,
                grey(band(4.0, row)),
                6,
                "accent span is thick",
            )?;
            check(
                &accent_frame,
                left + 60,
                row,
                grey(band(border, row)),
                6,
                "border between spans",
            )?;
        }
        // Row 22 lies inside the accent only; its butt end crosses column 40's centre.
        for (offset, coverage) in [(19, 1.0), (20, 0.5), (21, 0.0)] {
            check(
                &accent_frame,
                left + offset,
                22,
                grey(coverage),
                6,
                "accent butt end ramp",
            )?;
        }
        check(
            &accent_frame,
            left + 2,
            30,
            [255; 4],
            0,
            "accent along the left edge",
        )?;
        check(
            &accent_frame,
            left + 2,
            60,
            BLACK,
            0,
            "left edge between spans",
        )?;
    }

    // Strokes: a half-pixel line keeps half coverage instead of a full ramp's
    // contrast, butt ends ramp over one pixel, and two crossing translucent segments
    // blend once where they overlap.
    let stroke = |placement: [f32; 4], fill: [f32; 4], thickness: f32, segments| {
        ProbeBox {
            placement: placement.map(|length| length / 80.0),
            fill,
            border_color: [0.0; 4],
            corner: [0.0, 0.0],
            border: thickness / 80.0,
        }
        .records_shaped(
            CanvasShapeFill::Solid(fill),
            None,
            CanvasBoxShape::Stroke {
                segments,
            },
        )
    };
    let horizontal = [[0.0, 0.5, 1.0, 0.5], [0.0; 4]];
    let cross = [[0.0, 0.0, 1.0, 1.0], [0.0, 1.0, 1.0, 0.0]];
    let stroke_records = [
        stroke([20.0, 140.0, 100.0, 1.0], white, 0.5, horizontal),
        stroke([20.0, 160.0, 100.5, 4.0], white, 4.0, horizontal),
        stroke([180.0, 130.0, 60.0, 60.0], [1.0, 1.0, 1.0, 0.5], 6.0, cross),
    ]
    .concat();
    assert_eq!(stroke_records.len(), 3, "each stroke is one quad");
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(&mut device, &box_program, &stroke_records, &MVP, &ROOT)?;
    let stroke_frame = capture(&mut device, "shapes-strokes")?;
    for column in [40, 60, 80] {
        check(
            &stroke_frame,
            column,
            140,
            grey(0.5),
            6,
            "half-pixel stroke keeps its area",
        )?;
        check(&stroke_frame, column, 139, BLACK, 0, "above a thin stroke")?;
        check(&stroke_frame, column, 141, BLACK, 0, "below a thin stroke")?;
    }
    for (column, coverage) in [(119, 1.0), (120, 0.5), (121, 0.0)] {
        check(
            &stroke_frame,
            column,
            161,
            grey(coverage),
            6,
            "stroke butt end ramp",
        )?;
    }
    for (x, y, expected, label) in [
        (209, 159, grey(0.5), "crossing segments blend once"),
        (190, 140, grey(0.5), "one segment's arm"),
        (200, 140, BLACK, "between the arms"),
    ] {
        check(&stroke_frame, x, y, expected, 6, label)?;
    }

    // Arcs: rings of 40-pixel outer radius, their sectors in turns clockwise from
    // twelve o'clock. Centres sit on pixel corners or centres so that contours
    // cross pixel centres where a ramp is checked.
    let arc_part = |left: f32, top: f32, thickness: f32, fill: [f32; 4], shape| {
        ProbeBox {
            placement: [px(left), px(top), px(80.0), px(80.0)],
            fill,
            border_color: [0.0; 4],
            corner: [0.0, 0.0],
            border: px(thickness),
        }
        .records_shaped(CanvasShapeFill::Solid(fill), None, shape)
    };
    let arc = |start: f32, sweep: f32, dashes: f32, dash_duty: f32| CanvasBoxShape::Arc {
        start,
        sweep,
        dashes,
        dash_duty,
    };
    let linear = |frame: &[u8], x: u32, y: u32, channel: usize| {
        let encoded = f32::from(pixel(frame, x, y)[channel]) / 255.0;
        if encoded <= 0.040_45 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        }
    };
    let arc_records = [
        // A half-pixel whole ring centred on (60, 60).
        arc_part(20.0, 20.0, 0.5, white, arc(0.3, 1.0, 0.0, 1.0)),
        // A quarter from three to six o'clock, 8 pixels thick, centred on
        // (180.5, 60.5): its ends cross the centres of row 60 and column 180.
        arc_part(140.5, 20.5, 8.0, white, arc(0.25, 0.25, 0.0, 1.0)),
        // Eight dashes of half a cell on a whole ring centred on (60, 180), the
        // gaps centred on twelve, three, six and nine o'clock.
        arc_part(20.0, 140.0, 8.0, white, arc(0.0, 1.0, 8.0, 0.5)),
        // 360 dashes a quarter of a cell wide, 0.63 pixels a cell at the mean
        // radius: finer than the pixels, centred on (180, 180).
        arc_part(140.0, 140.0, 8.0, white, arc(0.0, 1.0, 360.0, 0.25)),
        // A sweep across twelve o'clock and a zero sweep, centred on (280, 60).
        arc_part(240.0, 20.0, 8.0, white, arc(0.9, 0.2, 0.0, 1.0)),
        arc_part(240.0, 140.0, 8.0, white, arc(0.4, 0.0, 0.0, 1.0)),
    ]
    .concat();
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(&mut device, &box_program, &arc_records, &MVP, &ROOT)?;
    let arc_frame = capture(&mut device, "shapes-arcs")?;
    // The half-pixel ring's band [99.5, 100] lies in column 99's right half and
    // [20, 20.5] in row 20's top half: half coverage, its area kept.
    for (x, y, expected, label) in [
        (
            99,
            60,
            grey(0.5),
            "half-pixel ring keeps its area at three o'clock",
        ),
        (98, 60, BLACK, "inside a half-pixel ring"),
        (100, 60, BLACK, "outside a half-pixel ring"),
        (
            60,
            20,
            grey(0.5),
            "half-pixel ring keeps its area at twelve o'clock",
        ),
        (60, 60, BLACK, "the ring's centre stays clear"),
    ] {
        check(&arc_frame, x, y, expected, 6, label)?;
    }
    // Butt ends ramp over one pixel across their radial lines.
    for (x, y, coverage, label) in [
        (216, 59, 0.0, "beyond the three o'clock end"),
        (216, 60, 0.5, "three o'clock end ramp"),
        (216, 61, 1.0, "inside the three o'clock end"),
        (179, 96, 0.0, "beyond the six o'clock end"),
        (180, 96, 0.5, "six o'clock end ramp"),
        (181, 96, 1.0, "inside the six o'clock end"),
        (180 + 25, 60 + 25, 1.0, "quarter body"),
        (180 - 25, 60 + 25, 0.0, "outside the quarter's sweep"),
        (180 + 18, 60 + 18, 0.0, "the quarter's hollow"),
    ] {
        check(&arc_frame, x, y, grey(coverage), 6, label)?;
    }
    // Dashes centred on 22.5 degrees and gaps on twelve and three o'clock, 36
    // pixels out.
    let on_ring = |center: [f32; 2], radius: f32, turns: f32| {
        let angle = turns * std::f32::consts::TAU;
        (
            (center[0] + radius * angle.sin()) as u32,
            (center[1] - radius * angle.cos()) as u32,
        )
    };
    for (turns, expected, label) in [
        (1.0 / 16.0, [255; 4], "on a dash"),
        (3.0 / 16.0, [255; 4], "on the next dash"),
        (0.0, BLACK, "in the gap at twelve o'clock"),
        (0.25, BLACK, "in the gap at three o'clock"),
    ] {
        let (x, y) = on_ring([60.0, 180.0], 36.0, turns);
        check(&arc_frame, x, y, expected, 6, label)?;
    }
    // Every pixel well inside the fine dashes' band, 1.6 cells across, shows their
    // duty instead of aliasing to dash or gap or beating between neighbours.
    let mut fine_dashes = 0;
    for y in 136..224 {
        for x in 136..224 {
            let radius = (x as f32 + 0.5 - 180.0).hypot(y as f32 + 0.5 - 180.0);
            if (radius - 36.0).abs() > 2.5 {
                continue;
            }
            let coverage = linear(&arc_frame, x, y, 0);
            if !(0.23..=0.27).contains(&coverage) {
                return Err(format!(
                    "fine dashes alias at ({x},{y}): coverage {coverage:.3}, duty 0.25"
                )
                .into());
            }
            fine_dashes += 1;
        }
    }
    if fine_dashes < 400 {
        return Err(format!("only {fine_dashes} fine dash samples").into());
    }
    // The sweep from 0.9 over twelve o'clock to 0.1 paints across the wrap; the zero
    // sweep paints nothing.
    for (turns, coverage, label) in [
        (0.0, 1.0, "across the wrap at twelve o'clock"),
        (0.95, 1.0, "before the wrap"),
        (0.05, 1.0, "after the wrap"),
        (0.15, 0.0, "beyond the wrapped sweep"),
    ] {
        let (x, y) = on_ring([280.0, 60.0], 36.0, turns);
        check(&arc_frame, x, y, grey(coverage), 6, label)?;
    }
    for turns in [0.0, 0.4, 0.75] {
        let (x, y) = on_ring([280.0, 180.0], 36.0, turns);
        check(&arc_frame, x, y, BLACK, 0, "a zero sweep paints nothing")?;
    }

    // Glow around an arc follows its exact distance: across a butt end, round the
    // end's outer corner, and into the hole beside the inner circle.
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &ProbeBox {
            placement: [px(140.5), px(20.5), px(80.0), px(80.0)],
            fill: [0.0, 0.0, 1.0, 1.0],
            border_color: [0.0; 4],
            corner: [0.0, 0.0],
            border: px(8.0),
        }
        .records_shaped(
            CanvasShapeFill::Solid([0.0, 0.0, 1.0, 1.0]),
            Some(&CanvasShapeGlow {
                color: [0.0, 1.0, 0.0, 1.0],
                intensity: 1.0,
                radius: px(20.0),
                inner_radius: 0.0,
                falloff: 1.0,
            }),
            arc(0.25, 0.25, 0.0, 1.0),
        ),
        &MVP,
        &ROOT,
    )?;
    let arc_glow_frame = capture(&mut device, "shapes-arc-glow")?;
    let from_center = |x: u32, y: u32| (centre(x) - 180.5).hypot(centre(y) - 60.5);
    for (x, y, distance, label) in [
        (
            216,
            50,
            60.5 - centre(50),
            "glow beyond the three o'clock end",
        ),
        (
            226,
            55,
            (centre(226) - 220.5).hypot(centre(55) - 60.5),
            "glow rounds the end's outer corner",
        ),
        (200, 80, 32.0 - from_center(200, 80), "glow into the hollow"),
        (
            212,
            92,
            from_center(212, 92) - 40.0,
            "glow beyond the outer circle",
        ),
    ] {
        check(
            &arc_glow_frame,
            x,
            y,
            [0, srgb(1.0 - distance / 20.0), 0, 255],
            4,
            label,
        )?;
    }

    // Oblique perspective: covered areas follow the independently projected cut
    // polygon, stroke rectangle and ring sector.
    let oblique_shapes = view::surface(
        &view::multiply(&view::rotation_y(0.6), &view::rotation_x(-0.35)),
        4.5,
    );
    let red_cut = ProbeBox {
        placement: [0.4, 0.4, 1.4, 1.0],
        fill: [1.0, 0.0, 0.0, 1.0],
        border_color: [0.0; 4],
        corner: [0.0, 0.0],
        border: 0.0,
    };
    let oblique_cut = CanvasBoxShape::Rect {
        corner_cut: [0.3, 0.0, 0.3, 0.0],
        corner_accent: [0.0; 4],
        corner_accent_width: 0.0,
        checker: None,
    };
    let green_stroke = ProbeBox {
        placement: [2.2, 0.4, 1.4, 1.0],
        fill: [0.0, 1.0, 0.0, 1.0],
        border: 0.12,
        ..red_cut
    };
    let diagonal = [[0.1, 0.2, 0.9, 0.8], [0.0; 4]];
    // A knob's 270-degree track centred on (2, 2.2), 0.6 out and 0.15 thick.
    let blue_arc = ProbeBox {
        placement: [1.4, 1.6, 1.2, 1.2],
        fill: [0.0, 0.0, 1.0, 1.0],
        border: 0.15,
        ..red_cut
    };
    let oblique_records = [
        red_cut.records_shaped(CanvasShapeFill::Solid(red_cut.fill), None, oblique_cut),
        green_stroke.records_shaped(
            CanvasShapeFill::Solid(green_stroke.fill),
            None,
            CanvasBoxShape::Stroke {
                segments: diagonal,
            },
        ),
        blue_arc.records_shaped(
            CanvasShapeFill::Solid(blue_arc.fill),
            None,
            arc(0.625, 0.75, 0.0, 1.0),
        ),
    ]
    .concat();
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &oblique_records,
        &oblique_shapes,
        &ROOT,
    )?;
    let oblique_frame = capture(&mut device, "shapes-perspective")?;
    let projected = |points: &[[f32; 2]]| {
        view::area(
            &points
                .iter()
                .map(|point| view::project(&oblique_shapes, *point).0)
                .collect::<Vec<_>>(),
        )
    };
    let cut_area = projected(&[
        [0.7, 0.4],
        [1.8, 0.4],
        [1.8, 1.1],
        [1.5, 1.4],
        [0.4, 1.4],
        [0.4, 0.7],
    ]);
    let [start, end] = [[2.34_f32, 0.6_f32], [3.46, 1.2]];
    let axis = [end[0] - start[0], end[1] - start[1]];
    let length = axis[0].hypot(axis[1]);
    let normal = [-axis[1] / length * 0.06, axis[0] / length * 0.06];
    let stroke_area = projected(&[
        [start[0] + normal[0], start[1] + normal[1]],
        [end[0] + normal[0], end[1] + normal[1]],
        [end[0] - normal[0], end[1] - normal[1]],
        [start[0] - normal[0], start[1] - normal[1]],
    ]);
    // Summed linear coverage measures area including antialiased edge pixels, which
    // a threshold count would overstate for a stroke a few pixels wide.
    let covered = |channel: usize| {
        (0..HEIGHT)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .map(|(x, y)| {
                let encoded = f32::from(pixel(&oblique_frame, x, y)[channel]) / 255.0;
                if encoded <= 0.040_45 {
                    encoded / 12.92
                } else {
                    ((encoded + 0.055) / 1.055).powf(2.4)
                }
            })
            .sum::<f32>()
    };
    // The sector's outline as a fine polygon: the outer circle forward over the
    // sweep, then the inner circle back.
    let sector: Vec<[f32; 2]> = [0.6_f32, 0.45]
        .into_iter()
        .enumerate()
        .flat_map(|(side, radius)| {
            (0..=96).map(move |step| {
                let step = if side == 0 {
                    step
                } else {
                    96 - step
                };
                let angle = (0.625 + 0.75 * step as f32 / 96.0) * std::f32::consts::TAU;
                [2.0 + radius * angle.sin(), 2.2 - radius * angle.cos()]
            })
        })
        .collect();
    let shape_regions = [
        ("cut", covered(0), cut_area),
        ("stroke", covered(1), stroke_area),
        ("arc", covered(2), projected(&sector)),
    ];
    for (label, covered, expected) in shape_regions {
        if (covered - expected).abs() > 0.05 * expected {
            return Err(format!(
                "perspective {label} region covers {covered:.1} px, expected {expected:.1}"
            )
            .into());
        }
    }

    // Colour fields and the checker. Expected colours come from the textbook HSV
    // model in sRGB levels at each pixel's centre and from linear-light composition
    // of the authored colours, encoded by the sRGB transfer function.
    let hsv = |hue: f32, saturation: f32, value: f32| -> [f32; 3] {
        let sector = hue.rem_euclid(1.0) * 6.0;
        let index = sector.floor();
        let f = sector - index;
        let p = value * (1.0 - saturation);
        let q = value * (1.0 - saturation * f);
        let t = value * (1.0 - saturation * (1.0 - f));
        let rgb = match index as u32 % 6 {
            0 => [value, t, p],
            1 => [q, value, p],
            2 => [p, value, t],
            3 => [p, q, value],
            4 => [t, p, value],
            _ => [value, p, q],
        };
        rgb.map(|channel| channel * 255.0)
    };
    let level = |linear: f32| {
        (if linear <= 0.003_130_8 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1.0 / 2.4) - 0.055
        }) * 255.0
    };
    let level_error = |frame: &[u8], x: u32, y: u32, expected: [f32; 3]| {
        let actual = pixel(frame, x, y);
        (0..3)
            .map(|channel| (f32::from(actual[channel]) - expected[channel]).abs())
            .fold(0.0, f32::max)
    };
    let field_part = |placement: [f32; 4], fill: CanvasShapeFill, shape: CanvasBoxShape| {
        ProbeBox {
            placement,
            fill: [0.0; 4],
            border_color: [0.0; 4],
            corner: [0.0, 0.0],
            border: 0.0,
        }
        .records_shaped(fill, None, shape)
    };
    let checkered = |size: f32, colors: [[f32; 4]; 2]| CanvasBoxShape::Rect {
        corner_cut: [0.0; 4],
        corner_accent: [0.0; 4],
        corner_accent_width: 0.0,
        checker: Some(CanvasShapeChecker {
            size,
            colors,
        }),
    };
    let clear = CanvasShapeFill::Solid([0.0; 4]);
    let light = [0.6, 0.6, 0.6, 1.0];
    let dark = [0.1, 0.1, 0.1, 1.0];
    let black_white = [[1.0; 4], [0.0, 0.0, 0.0, 1.0]];
    let cyan = [0.1, 0.9, 1.0];
    let field_records = [
        field_part(
            [px(20.0), px(20.0), px(80.0), px(80.0)],
            CanvasShapeFill::SaturationValue {
                hue: 0.55,
            },
            CanvasBoxShape::RECT,
        ),
        // Red at its bottom, rising upward.
        field_part(
            [px(120.0), px(20.0), px(16.0), px(80.0)],
            CanvasShapeFill::Hue {
                start: [0.0, px(80.0)],
                end: [0.0, 0.0],
            },
            CanvasBoxShape::RECT,
        ),
        field_part(
            [px(150.0), px(20.0), px(48.0), px(48.0)],
            clear,
            checkered(px(8.0), [light, dark]),
        ),
        // Cells of 0.4 pixels.
        field_part(
            [px(210.0), px(20.0), px(40.0), px(40.0)],
            clear,
            checkered(px(0.4), black_white),
        ),
        // An alpha ramp of one colour, transparent at its left, over 8-pixel cells.
        field_part(
            [px(20.0), px(120.0), px(160.0), px(32.0)],
            CanvasShapeFill::LinearGradient {
                start: [0.0, 0.0],
                end: [px(160.0), 0.0],
                start_color: [cyan[0], cyan[1], cyan[2], 0.0],
                end_color: [cyan[0], cyan[1], cyan[2], 1.0],
            },
            checkered(px(8.0), [light, dark]),
        ),
    ]
    .concat();
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(&mut device, &box_program, &field_records, &MVP, &ROOT)?;
    let field_frame = capture(&mut device, "colour-fields")?;
    let mut field_error = 0.0f32;
    let mut rail_error = 0.0f32;
    for y in 21..99 {
        for x in 21..99 {
            let expected = hsv(
                0.55,
                (centre(x) - 20.0) / 80.0,
                1.0 - (centre(y) - 20.0) / 80.0,
            );
            field_error = field_error.max(level_error(&field_frame, x, y, expected));
        }
        for x in 121..135 {
            let expected = hsv((100.0 - centre(y)) / 80.0, 1.0, 1.0);
            rail_error = rail_error.max(level_error(&field_frame, x, y, expected));
        }
    }
    let mut checker_error = 0.0f32;
    let cell_colour = |origin: [f32; 2], cell: f32, colors: [[f32; 4]; 2], x: f32, y: f32| {
        colors[(((x - origin[0]) / cell).floor() + ((y - origin[1]) / cell).floor()).rem_euclid(2.0)
            as usize]
    };
    for row in 0..6 {
        for column in 0..6 {
            let [x, y] = [150 + column * 8 + 4, 20 + row * 8 + 4];
            let color = cell_colour([150.0, 20.0], 8.0, [light, dark], centre(x), centre(y));
            checker_error =
                checker_error.max(level_error(&field_frame, x, y, [level(color[0]); 3]));
        }
    }
    let mut fine_error = 0.0f32;
    for y in 21..59 {
        for x in 211..249 {
            fine_error = fine_error.max(level_error(&field_frame, x, y, [level(0.5); 3]));
        }
    }
    let mut alpha_error = 0.0f32;
    for row in 0..4 {
        for column in 0..20 {
            let [x, y] = [20 + column * 8 + 4, 120 + row * 8 + 4];
            let alpha = (centre(x) - 20.0) / 160.0;
            let under = cell_colour([20.0, 120.0], 8.0, [light, dark], centre(x), centre(y));
            let expected = std::array::from_fn(|channel| {
                level(cyan[channel] * alpha + under[channel] * (1.0 - alpha))
            });
            alpha_error = alpha_error.max(level_error(&field_frame, x, y, expected));
        }
    }
    println!(
        "colour fields: largest error in sRGB levels: saturation-value {field_error:.2}, hue {rail_error:.2}, checker cells {checker_error:.2}, sub-pixel checker {fine_error:.2}, alpha over checker {alpha_error:.2}"
    );
    for (label, error) in [
        ("saturation-value field", field_error),
        ("hue rail", rail_error),
        ("checker cells", checker_error),
        ("sub-pixel checker mean", fine_error),
        ("alpha ramp over the checker", alpha_error),
    ] {
        if error > 1.5 {
            return Err(format!("{label} differs from its model by {error:.2} sRGB levels").into());
        }
    }

    // On a steeply tilted Surface the checker filters by each pixel's footprint: where
    // it spans a cell or more along either axis the pixel shows the colours' mean,
    // never a cell or a beat between neighbours, and where cells span several pixels
    // their colours stay. The oracle inverts the projection to find each pixel's
    // Surface point and footprint.
    let tilted = view::surface(&view::rotation_x(-1.1), 3.4);
    let checker_boxes = [
        ([0.2, 0.2, 1.7, 2.6], 0.25, [light, dark]),
        ([2.1, 0.2, 1.7, 2.6], 0.02, black_white),
    ];
    let checker_records: Vec<_> = checker_boxes
        .iter()
        .flat_map(|(placement, cell, colors)| {
            field_part(*placement, clear, checkered(*cell, *colors))
        })
        .collect();
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(&mut device, &box_program, &checker_records, &tilted, &ROOT)?;
    let tilted_frame = capture(&mut device, "checker-perspective")?;
    // Pixel (px, py) times w is a projective map of Surface (x, y, 1); its inverse
    // takes a pixel centre back to the Surface.
    let [width, height] = [WIDTH as f32, HEIGHT as f32];
    let m = tilted;
    let forward = [
        [
            (m[0] + m[3]) * width / 2.0,
            (m[4] + m[7]) * width / 2.0,
            (m[12] + m[15]) * width / 2.0,
        ],
        [
            (m[3] - m[1]) * height / 2.0,
            (m[7] - m[5]) * height / 2.0,
            (m[15] - m[13]) * height / 2.0,
        ],
        [m[3], m[7], m[15]],
    ];
    let inverse = {
        let a = forward.map(|row| row.map(f64::from));
        let cofactor = |r0: usize, r1: usize, c0: usize, c1: usize| {
            a[r0][c0] * a[r1][c1] - a[r0][c1] * a[r1][c0]
        };
        let adjugate = [
            [
                cofactor(1, 2, 1, 2),
                -cofactor(0, 2, 1, 2),
                cofactor(0, 1, 1, 2),
            ],
            [
                -cofactor(1, 2, 0, 2),
                cofactor(0, 2, 0, 2),
                -cofactor(0, 1, 0, 2),
            ],
            [
                cofactor(1, 2, 0, 1),
                -cofactor(0, 2, 0, 1),
                cofactor(0, 1, 0, 1),
            ],
        ];
        let determinant: f64 = (0..3)
            .map(|column| a[0][column] * adjugate[column][0])
            .sum();
        adjugate.map(|row| row.map(|value| value / determinant))
    };
    let surface_point = |x: f64, y: f64| {
        let [u, v, w] = inverse.map(|row| row[0] * x + row[1] * y + row[2]);
        [u / w, v / w]
    };
    // The forward projection agrees with the oracle's own.
    let probe = view::project(&tilted, [1.0, 1.0]).0;
    let back = surface_point(f64::from(probe[0]), f64::from(probe[1]));
    if (back[0] - 1.0).abs() > 1e-3 || (back[1] - 1.0).abs() > 1e-3 {
        return Err(format!("checker oracle inverse projection is {back:?}").into());
    }
    let (mut minified, mut magnified, mut tilted_error) = (0u32, 0u32, 0.0f32);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let center = [f64::from(x) + 0.5, f64::from(y) + 0.5];
            let point = surface_point(center[0], center[1]);
            let right = surface_point(center[0] + 1.0, center[1]);
            let down = surface_point(center[0], center[1] + 1.0);
            let footprint = [0, 1]
                .map(|axis| (right[axis] - point[axis]).abs() + (down[axis] - point[axis]).abs());
            for ([left, top, box_width, box_height], cell, colors) in checker_boxes {
                let origin = [f64::from(left), f64::from(top)];
                let extent = [f64::from(box_width), f64::from(box_height)];
                // Clear of the box's antialiased edges.
                let inside = (0..2).all(|axis| {
                    let offset = point[axis] - origin[axis];
                    offset > 3.0 * footprint[axis] && offset < extent[axis] - 3.0 * footprint[axis]
                });
                if !inside {
                    continue;
                }
                let cell = f64::from(cell);
                let widths = footprint.map(|extent| extent / cell);
                let expected = if widths[0].max(widths[1]) >= 1.25 {
                    minified += 1;
                    std::array::from_fn(|channel| {
                        level((colors[0][channel] + colors[1][channel]) * 0.5)
                    })
                } else if widths[0].max(widths[1]) <= 0.25
                    && (0..2).all(|axis| {
                        let offset = (point[axis] - origin[axis]) / cell;
                        let to_edge = (offset - offset.round()).abs();
                        to_edge > widths[axis]
                    })
                {
                    magnified += 1;
                    let color = cell_colour(
                        [0.0, 0.0],
                        cell as f32,
                        colors,
                        (point[0] - origin[0]) as f32,
                        (point[1] - origin[1]) as f32,
                    );
                    std::array::from_fn(|channel| level(color[channel]))
                } else {
                    continue;
                };
                let error = level_error(&tilted_frame, x, y, expected);
                if error > 1.5 {
                    return Err(format!(
                        "tilted checker at ({x},{y}), footprint {widths:?} cells: {:?}, expected {expected:?}",
                        pixel(&tilted_frame, x, y)
                    )
                    .into());
                }
                tilted_error = tilted_error.max(error);
            }
        }
    }
    println!(
        "tilted checker: {minified} minified pixels at the mean, {magnified} magnified cell pixels, largest error {tilted_error:.2} sRGB levels"
    );
    if minified < 500 || magnified < 500 {
        return Err(format!(
            "tilted checker classified only {minified} minified and {magnified} magnified pixels"
        )
        .into());
    }

    // Grazing views: nearly edge-on, the padded geometry of filled and sparse boxes
    // stays within a few pixels of the box silhouette instead of stretching toward
    // the camera, and the visible sliver keeps continuous coverage.
    let footprint_program = device.create_program(
        include_str!("../src/services/render/shaders/surface_gui.vert"),
        FOOTPRINT_FRAGMENT,
    )?;
    let grazing_fill = ProbeBox {
        placement: [0.5, 0.5, 3.0, 2.0],
        fill: [1.0; 4],
        border_color: [1.0; 4],
        corner: [0.0, 0.0],
        border: 0.0,
    };
    let grazing_ring = ProbeBox {
        fill: [0.0; 4],
        border: 0.1,
        ..grazing_fill
    };
    assert_eq!(grazing_ring.records().len(), 4);

    let mut grazing_report = Vec::new();
    for degrees in [88.0_f32, 89.5, 89.9] {
        let mvp = view::surface(&view::rotation_y(degrees.to_radians()), 4.0);
        let silhouette = view::corners(&mvp, grazing_fill.placement);
        let [min_x, max_x, min_y, max_y] = view::bounds(&silhouette);
        for (label, probe) in [("fill", grazing_fill), ("ring", grazing_ring)] {
            device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
            draw_batch(
                &mut device,
                &footprint_program,
                &probe.records(),
                &mvp,
                &ROOT,
            )?;
            let footprint = capture(&mut device, &format!("boxes-grazing-{label}-{degrees}"))?;

            let mut rasterized = 0;
            let mut beyond = 0.0_f32;
            for y in 0..HEIGHT {
                for x in 0..WIDTH {
                    if pixel(&footprint, x, y) == [255, 0, 255, 255] {
                        let [centre_x, centre_y] = [x as f32 + 0.5, y as f32 + 0.5];
                        rasterized += 1;
                        beyond = beyond
                            .max(min_x - centre_x)
                            .max(centre_x - max_x)
                            .max(min_y - centre_y)
                            .max(centre_y - max_y);
                    }
                }
            }
            if beyond > 12.0 {
                return Err(format!(
                    "grazing {label} geometry at {degrees} degrees reaches {beyond:.1} px beyond its silhouette"
                )
                .into());
            }
            grazing_report.push(format!(
                "{label}@{degrees}: rasterized={rasterized} beyond={beyond:.1} silhouette_area={:.1}",
                view::area(&silhouette)
            ));
        }
    }
    device.delete_program(footprint_program);

    let sliver_mvp = view::surface(&view::rotation_y(89.5_f32.to_radians()), 4.0);
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &grazing_fill.records(),
        &sliver_mvp,
        &ROOT,
    )?;
    let sliver = capture(&mut device, "boxes-grazing-sliver")?;

    let silhouette = view::corners(&sliver_mvp, grazing_fill.placement);
    let [min_x, max_x, _, _] = view::bounds(&silhouette);

    // Corners run near-top, far-top, far-bottom, near-bottom: rows between the far
    // edge's ends cross both projected edges of the nearly edge-on box.
    let rows = silhouette[1][1].ceil() as u32 + 2..silhouette[2][1].floor() as u32 - 2;
    let columns = min_x.floor() as u32 - 2..=max_x.ceil() as u32 + 2;
    for row in rows {
        let brightest = columns
            .clone()
            .map(|column| pixel(&sliver, column, row)[0])
            .max()
            .unwrap_or(0);
        if brightest < 128 {
            return Err(format!("grazing sliver has a coverage gap in row {row}").into());
        }
    }

    let path_program = device.create_program(
        include_str!("../src/services/render/shaders/surface.vert"),
        include_str!("../src/services/render/shaders/surface.frag"),
    )?;
    let (square, square_descriptor) = unit_square(&mut device)?;

    let text_program = device.create_program(
        include_str!("../src/services/render/shaders/surface_glyph.vert"),
        include_str!("../src/services/render/shaders/surface_glyph.frag"),
    )?;
    let page = device.create_glyph_atlas_page(512, 512)?;

    // Draw white unit square into slot [1, 1] to [31, 31] on the atlas page.
    device.begin_glyph_atlas_page(&page)?;
    let page_dim = 512.0;
    let atlas_mvp = [
        2.0 / page_dim,
        0.0,
        0.0,
        0.0,
        0.0,
        -2.0 / page_dim,
        0.0,
        0.0,
        0.0,
        0.0,
        1.0,
        0.0,
        -1.0,
        1.0,
        0.0,
        1.0,
    ];
    let slot_scale = 30.0;
    let slot_placement = [1.0, 1.0, slot_scale, slot_scale];
    let atlas_clip = [0.0, 0.0, page_dim, page_dim];
    let white = [1.0, 1.0, 1.0, 1.0];
    device.draw_surface_path(
        &path_program,
        &square,
        &[0.0, 0.0, 1.0, 1.0],
        square_descriptor,
        &atlas_mvp,
        &slot_placement,
        &atlas_clip,
        &white,
        0,
    )?;
    device.end_glyph_atlas_page()?;

    let u0 = 1.0 / 512.0;
    let v0 = 1.0 - 1.0 / 512.0;
    let u1 = 31.0 / 512.0;
    let v1 = 1.0 - 31.0 / 512.0;
    let cyan = [0.0, 1.0, 1.0, 1.0];
    let glyph_records = [glyph_record([1.0, 1.0, 2.0, 2.0], [u0, v0, u1, v1], cyan)];
    let mut glyph_batch = upload_records(&mut device, &glyph_records)?;

    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    let atlas_tex = *ipp_render_gl::GlesRenderDevice::glyph_atlas_texture(&page);
    device.draw_gui_batch(&text_program, &glyph_batch, Some(&atlas_tex), &MVP, 0, 1)?;
    let text_batch_frame = capture(&mut device, "glyph-batch")?;
    check(
        &text_batch_frame,
        120,
        120,
        [0, 255, 255, 255],
        8,
        "retained glyph batch quad cyan tint",
    )?;

    // Rewrite the stored quad with a yellow tint
    let yellow = [1.0, 1.0, 0.0, 1.0];
    let updated_records = [glyph_record([1.0, 1.0, 2.0, 2.0], [u0, v0, u1, v1], yellow)];
    device.write_gui_batch(&mut glyph_batch, 0, &updated_records)?;

    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_gui_batch(&text_program, &glyph_batch, Some(&atlas_tex), &MVP, 0, 1)?;
    let updated_batch_frame = capture(&mut device, "glyph-batch-updated")?;
    check(
        &updated_batch_frame,
        120,
        120,
        [255, 255, 0, 255],
        8,
        "retained glyph batch updated yellow tint",
    )?;

    // A shape and a glyph whose records carry different clips: the box keeps only its
    // left part and the glyph quad only its lower half.
    let clipped_box = ProbeBox {
        placement: [2.25, 0.25, 1.5, 1.0],
        fill: [1.0, 0.0, 0.0, 1.0],
        border_color: [1.0; 4],
        corner: [0.0, 0.0],
        border: 0.0,
    };
    let box_clip = [2.25, 0.0, 3.0, 3.0];
    let glyph_clip = [0.0, 1.5, 4.0, 3.0];
    let clipped_shapes = clipped_box.records();
    let clipped_glyphs = [ipp_render_gl::GuiGlyphRecord {
        clip: glyph_clip,
        ..glyph_records[0]
    }];
    let shape_batch = upload(&mut device, &clipped_shapes, &box_clip)?;
    let text_batch = upload_records(&mut device, &clipped_glyphs)?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_gui_batch(
        &box_program,
        &shape_batch,
        None,
        &MVP,
        0,
        clipped_shapes.len(),
    )?;
    device.draw_gui_batch(&text_program, &text_batch, Some(&atlas_tex), &MVP, 0, 1)?;
    let mixed = capture(&mut device, "mixed-clips")?;
    device.delete_gui_batch(shape_batch);
    device.delete_gui_batch(text_batch);
    for (x, y, expected, label) in [
        (208, 60, [255, 0, 0, 255], "box inside its clip"),
        (272, 60, [0, 0, 0, 255], "box beyond its clip"),
        (120, 140, [0, 255, 255, 255], "glyph inside its clip"),
        (120, 100, [0, 0, 0, 255], "glyph beyond its clip"),
    ] {
        check(&mixed, x, y, expected, 8, label)?;
    }

    // Perspective retained batches: an oblique Surface whose clip w varies across it
    // draws a filled rounded box, a sparse hollow border and an atlas glyph quad.
    let oblique = view::surface(
        &view::multiply(&view::rotation_y(0.6), &view::rotation_x(-0.35)),
        4.5,
    );
    let depths = [[0.0, 0.0], [4.0, 0.0], [4.0, 3.0], [0.0, 3.0]].map(|corner| {
        let (_, w) = view::project(&oblique, corner);
        w
    });
    let [nearest, farthest] = depths.iter().fold([f32::MAX, f32::MIN], |[low, high], w| {
        [low.min(*w), high.max(*w)]
    });
    if farthest - nearest < 1.0 {
        return Err(format!("perspective Surface must vary clip w: {depths:?}").into());
    }

    let filled = ProbeBox {
        placement: [0.4, 0.4, 1.4, 1.0],
        fill: [1.0, 0.0, 0.0, 1.0],
        border_color: [1.0; 4],
        corner: [0.15, 0.15],
        border: 0.0,
    };
    let outline = ProbeBox {
        placement: [2.2, 0.4, 1.4, 1.0],
        fill: [0.0; 4],
        border_color: [0.0, 1.0, 0.0, 1.0],
        corner: [0.0, 0.0],
        border: 0.12,
    };
    let outline_records = outline.records();
    assert_eq!(outline_records.len(), 4);

    let glyph_rectangle = [0.6, 1.8, 1.0, 0.8];
    let [left, top, right, bottom] = [
        glyph_rectangle[0],
        glyph_rectangle[1],
        glyph_rectangle[0] + glyph_rectangle[2],
        glyph_rectangle[1] + glyph_rectangle[3],
    ];
    let perspective_glyph = [glyph_record(
        [left, top, right, bottom],
        [u0, v0, u1, v1],
        cyan,
    )];
    // The boxes share one shape storage and draw as one range, then the glyph quad.
    let perspective_records = [filled.records(), outline_records].concat();
    let perspective_batch = upload(&mut device, &perspective_records, &ROOT)?;
    let perspective_text = upload_records(&mut device, &perspective_glyph)?;

    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_gui_batch(
        &box_program,
        &perspective_batch,
        None,
        &oblique,
        0,
        perspective_records.len(),
    )?;
    device.draw_gui_batch(
        &text_program,
        &perspective_text,
        Some(&atlas_tex),
        &oblique,
        0,
        1,
    )?;
    let perspective = capture(&mut device, "retained-perspective")?;

    let red = [255, 0, 0, 255];
    let green = [0, 255, 0, 255];
    for (point, expected, label) in [
        ([1.1, 0.9], red, "perspective filled box centre"),
        (
            [0.6, 0.6],
            red,
            "perspective filled box near its rounded corner",
        ),
        (
            [2.9, 0.9],
            BLACK,
            "perspective hollow border centre stays clear",
        ),
        ([2.9, 0.46], green, "perspective top border band"),
        ([2.26, 0.9], green, "perspective left border band"),
        ([3.54, 0.9], green, "perspective right border band"),
        (
            [1.1, 2.2],
            [0, 255, 255, 255],
            "perspective glyph quad centre",
        ),
        ([2.0, 2.2], BLACK, "perspective gap between primitives"),
    ] {
        let ([x, y], _) = view::project(&oblique, point);
        check(&perspective, x as u32, y as u32, expected, 12, label)?;
    }

    // Region areas follow the independently projected shapes: the rounded corners
    // remove (4 - pi) r^2 of the filled box, and the ring is the difference of its
    // outer and inner rectangles.
    let count = |matches: fn([u8; 4]) -> bool| {
        (0..HEIGHT)
            .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
            .filter(|&(x, y)| matches(pixel(&perspective, x, y)))
            .count() as f32
    };
    let filled_area = view::area(&view::corners(&oblique, filled.placement))
        * (1.0 - (4.0 - std::f32::consts::PI) * 0.15 * 0.15 / (1.4 * 1.0));
    let ring_area = view::area(&view::corners(&oblique, outline.placement))
        - view::area(&view::corners(&oblique, [2.32, 0.52, 1.16, 0.76]));
    let glyph_area = view::area(&view::corners(&oblique, glyph_rectangle));
    let perspective_regions = [
        (
            "filled",
            count(|pixel| pixel[0] > 128 && pixel[1] < 64 && pixel[2] < 64),
            filled_area,
            0.1,
        ),
        (
            "ring",
            count(|pixel| pixel[1] > 128 && pixel[0] < 64 && pixel[2] < 64),
            ring_area,
            0.2,
        ),
        (
            "glyph",
            count(|pixel| pixel[1] > 128 && pixel[2] > 128 && pixel[0] < 64),
            glyph_area,
            0.1,
        ),
    ];
    for (label, counted, expected, tolerance) in perspective_regions {
        if (counted - expected).abs() > tolerance * expected {
            return Err(format!(
                "perspective {label} region covers {counted} px, expected {expected:.0}"
            )
            .into());
        }
    }
    device.delete_gui_batch(perspective_batch);

    device.delete_gui_batch(glyph_batch);
    device.delete_glyph_atlas_page(page);
    device.delete_program(text_program);
    device.delete_surface_path(square);
    device.delete_program(path_program);
    device.delete_program(box_program);

    std::fs::write(
        evidence.join("gui-clips.txt"),
        format!(
            "nested=[{:?} {:?} {:?}]\ntext=[{:?} {:?}]\nbitmap=[{:?} {:?}]\ntilted_green={green_count}\nclose_border_profile={close_profile:?}\ngrazing=[{}]\nperspective_w=[{nearest:.3} {farthest:.3}]\nperspective_regions=[{}]\nperspective_shapes=[{}]\n{}\n",
            pixel(&nested, 120, 120),
            pixel(&nested, 60, 60),
            pixel(&nested, 240, 40),
            pixel(&text_before, 80, 92),
            pixel(&text_after, 80, 92),
            pixel(&bitmap_before, 120, 120),
            pixel(&bitmap_after, 120, 120),
            grazing_report.join("; "),
            perspective_regions
                .map(|(label, counted, expected, _)| format!("{label}={counted}/{expected:.0}"))
                .join(" "),
            shape_regions
                .map(|(label, covered, expected)| format!("{label}={covered:.1}/{expected:.1}"))
                .join(" "),
            context.info().unwrap_or_else(|_| "no device info".into()),
        ),
    )?;
    println!(
        "PASS: nested clips, scrolled primitives, boxes, order, coverage ramps, glow, cut corners, corner accents, inner glow, strokes, arcs, dashes, grazing and perspective views, Surface cache targets and recovery"
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The EGL GUI clip runner supports Linux; no graphics test was run.");
    std::process::exit(1);
}
