//! Focused native GLES clip and GUI box frame oracle.
//!
//! Device-level counterpart to the raw Surface scenes in `egl_surfaces`: nested
//! clip intersections, scrolled curves/text-proxies/bitmaps, parameterized
//! rounded boxes drawn as one-box retained batches with explicit per-axis
//! corner/border dimensions, empty-clip suppression, painter-order overlap, box
//! coverage ramps at close range and clip edges, premultiplied gradients, glow
//! falloff, tilted, grazing and perspective views, Surface cache targets with
//! nested atlas population and device replacement. All fixtures are synthetic
//! and local; assertions compare completed-frame pixels against values derived
//! independently from the scene layout and projection below.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod smoke;

#[cfg(target_os = "linux")]
use ipp_core::systems::surface::{
    GuiShapeFill, GuiShapeGlow, SurfaceItemId, SurfacePrimitiveIdentity, SurfacePrimitiveStyle,
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
    /// Production retained-batch vertices for this box with any fill and glow.
    fn vertices_with(
        &self,
        fill: GuiShapeFill,
        glow: Option<&GuiShapeGlow>,
    ) -> Vec<ipp_render_gl::GuiVertex> {
        let style = SurfacePrimitiveStyle {
            identity: SurfacePrimitiveIdentity::Authored(SurfaceItemId(0)),
            position: [self.placement[0], self.placement[1]],
            scale: [1.0, 1.0],
            color: [1.0; 4],
            opacity: 1.0,
            clip: None,
        };
        ipp_render_gl::gui_batch::generate_box_vertices(
            &style,
            &[self.placement[2], self.placement[3]],
            &self.corner,
            self.border,
            &self.border_color,
            &fill,
            glow,
            ROOT,
        )
    }

    fn vertices(&self) -> Vec<ipp_render_gl::GuiVertex> {
        self.vertices_with(GuiShapeFill::Solid(self.fill), None)
    }
}

/// Upload `vertices`, each clipped by `clip`, into new retained GUI storage.
#[cfg(target_os = "linux")]
fn upload<D: ipp_render_gl::RenderDevice>(
    device: &mut D,
    vertices: &[ipp_render_gl::GuiVertex],
    clip: &[f32; 4],
) -> Result<D::GuiBatch, Box<dyn std::error::Error>> {
    let clipped: Vec<_> = vertices
        .iter()
        .map(|vertex| ipp_render_gl::GuiVertex {
            clip: *clip,
            ..*vertex
        })
        .collect();
    let mut batch = device.create_gui_batch(clipped.len())?;
    if let Err(error) = device.write_gui_batch(&mut batch, 0, &clipped) {
        device.delete_gui_batch(batch);
        return Err(error.into());
    }
    Ok(batch)
}

/// Draw `vertices` as one retained batch: allocate, draw once and release.
#[cfg(target_os = "linux")]
fn draw_batch<D: ipp_render_gl::RenderDevice>(
    device: &mut D,
    program: &D::Program,
    vertices: &[ipp_render_gl::GuiVertex],
    mvp: &[f32; 16],
    clip: &[f32; 4],
) -> Result<(), Box<dyn std::error::Error>> {
    let batch = upload(device, vertices, clip)?;
    let drawn = device.draw_gui_batch(program, &batch, None, mvp, 0, vertices.len());
    device.delete_gui_batch(batch);
    Ok(drawn?)
}

/// Atlas glyph quad corner in the shared GUI vertex layout, clipped by `ROOT`.
#[cfg(target_os = "linux")]
fn glyph_vertex(position: [f32; 2], uv: [f32; 2], color: [f32; 4]) -> ipp_render_gl::GuiVertex {
    ipp_render_gl::GuiVertex {
        position,
        color0: color,
        gradient_coords: [uv[0], uv[1], 0.0, 0.0],
        material_params: [ipp_render_gl::gui_batch::GUI_FILL_GLYPH, 0.0, 0.0, 1.0],
        clip: ROOT,
        ..ipp_render_gl::GuiVertex::EMPTY
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
        draw_batch(device, box_program, &probe.vertices(), &MVP, &ROOT)?;
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
    use ipp_core::services::asset_management::quadratic::{QuadraticContour, QuadraticSegment};
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
        &suppressed_box.vertices(),
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
            &resized_box.vertices(),
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
            draw_batch(device, &box_program, &entry.vertices(), &MVP, &ROOT)?;
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
        &tilted_box.vertices(),
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
    let linear_style = ipp_core::systems::surface::SurfacePrimitiveStyle {
        identity: ipp_core::systems::surface::SurfacePrimitiveIdentity::Authored(
            ipp_core::systems::surface::SurfaceItemId(10),
        ),
        position: [0.5, 0.5],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: None,
    };
    let linear_fill = ipp_core::systems::surface::GuiShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [1.0, 1.0],
        start_color: [1.0, 0.0, 0.0, 1.0],
        end_color: [0.0, 0.0, 1.0, 1.0],
    };
    let mut batch_vertices = Vec::new();
    batch_vertices.extend_from_slice(&ipp_render_gl::gui_batch::generate_box_vertices(
        &linear_style,
        &[1.0, 1.0],
        &[0.1, 0.1],
        0.0,
        &[0.0; 4],
        &linear_fill,
        None,
        ROOT,
    ));

    let radial_style = ipp_core::systems::surface::SurfacePrimitiveStyle {
        identity: ipp_core::systems::surface::SurfacePrimitiveIdentity::Authored(
            ipp_core::systems::surface::SurfaceItemId(11),
        ),
        position: [2.0, 0.5],
        scale: [1.0, 1.0],
        color: [1.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: None,
    };
    let radial_fill = ipp_core::systems::surface::GuiShapeFill::RadialGradient {
        center: [0.5, 0.5],
        radius: 0.5,
        start_color: [1.0, 1.0, 0.0, 1.0],
        end_color: [0.5, 0.0, 0.5, 1.0],
    };
    batch_vertices.extend_from_slice(&ipp_render_gl::gui_batch::generate_box_vertices(
        &radial_style,
        &[1.0, 1.0],
        &[0.35, 0.12],
        0.0,
        &[0.0; 4],
        &radial_fill,
        None,
        ROOT,
    ));

    let glow_style = ipp_core::systems::surface::SurfacePrimitiveStyle {
        identity: ipp_core::systems::surface::SurfacePrimitiveIdentity::Authored(
            ipp_core::systems::surface::SurfaceItemId(12),
        ),
        position: [0.5, 1.75],
        scale: [1.0, 1.0],
        color: [0.0, 1.0, 1.0, 1.0],
        opacity: 1.0,
        clip: None,
    };
    let glow = ipp_core::systems::surface::GuiShapeGlow {
        color: [0.0, 1.0, 0.0, 1.0],
        intensity: 1.0,
        radius: 0.15,
        falloff: 1.5,
    };
    batch_vertices.extend_from_slice(&ipp_render_gl::gui_batch::generate_box_vertices(
        &glow_style,
        &[1.0, 0.75],
        &[0.1, 0.1],
        0.0,
        &[0.0; 4],
        &ipp_core::systems::surface::GuiShapeFill::Solid([0.0, 1.0, 1.0, 1.0]),
        Some(&glow),
        ROOT,
    ));

    let border_style = ipp_core::systems::surface::SurfacePrimitiveStyle {
        identity: ipp_core::systems::surface::SurfacePrimitiveIdentity::Authored(
            ipp_core::systems::surface::SurfaceItemId(13),
        ),
        position: [2.0, 1.75],
        scale: [1.0, 1.0],
        color: [0.0, 0.0, 0.0, 0.0],
        opacity: 1.0,
        clip: None,
    };
    let border_verts = ipp_render_gl::gui_batch::generate_box_vertices(
        &border_style,
        &[1.0, 0.75],
        &[0.1, 0.1],
        0.1,
        &[1.0, 0.5, 0.0, 1.0],
        &ipp_core::systems::surface::GuiShapeFill::Solid([0.0; 4]),
        None,
        ROOT,
    );
    assert_eq!(
        border_verts.len(),
        24,
        "border-only must use 4 edge strips (24 vertices)"
    );
    batch_vertices.extend_from_slice(&border_verts);

    // A small glowing outline uses a full quad, so the shader must keep its
    // hollow center clear independently of the sparse-outline optimization.
    let hollow_style = ipp_core::systems::surface::SurfacePrimitiveStyle {
        position: [3.4, 1.75],
        ..border_style
    };
    let hollow_vertices = ipp_render_gl::gui_batch::generate_box_vertices(
        &hollow_style,
        &[0.4, 0.4],
        &[0.05, 0.05],
        0.02,
        &[1.0, 0.5, 0.0, 1.0],
        &ipp_core::systems::surface::GuiShapeFill::Solid([0.0; 4]),
        Some(&glow),
        ROOT,
    );
    assert_eq!(hollow_vertices.len(), 6);
    batch_vertices.extend_from_slice(&hollow_vertices);

    let batch = upload(&mut device, &batch_vertices, &ROOT)?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_gui_batch(&box_program, &batch, None, &MVP, 0, batch_vertices.len())?;
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
    let rail_style = ipp_core::systems::surface::SurfacePrimitiveStyle {
        position: [0.5, 0.5],
        ..glow_style
    };
    let rail = ipp_render_gl::gui_batch::generate_box_vertices(
        &rail_style,
        &[1.0, 0.0125],
        &[0.0, 0.0],
        0.0,
        &[0.0; 4],
        &ipp_core::systems::surface::GuiShapeFill::Solid([1.0; 4]),
        Some(&glow),
        ROOT,
    );
    let outline_style = ipp_core::systems::surface::SurfacePrimitiveStyle {
        position: [2.0, 0.5],
        ..rail_style
    };
    let outline = ipp_render_gl::gui_batch::generate_box_vertices(
        &outline_style,
        &[1.0, 0.5],
        &[0.0, 0.0],
        0.0125,
        &[1.0; 4],
        &ipp_core::systems::surface::GuiShapeFill::Solid([0.0; 4]),
        Some(&glow),
        ROOT,
    );
    let sharp_vertices = [rail, outline].concat();
    let sharp_batch = upload(&mut device, &sharp_vertices, &ROOT)?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_gui_batch(
        &box_program,
        &sharp_batch,
        None,
        &MVP,
        0,
        sharp_vertices.len(),
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
    let hollow_vertices = hollow.vertices();
    assert_eq!(
        hollow_vertices.len(),
        24,
        "hollow border must use sparse strips"
    );

    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &hollow_vertices,
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
        &clipped_box.vertices(),
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
    let fading = GuiShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [2.0, 0.0],
        start_color: [1.0, 0.0, 0.0, 1.0],
        end_color: [0.0, 0.0, 1.0, 0.0],
    };
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &gradient_box.vertices_with(fading, None),
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
    let falloff_glow = GuiShapeGlow {
        color: [0.0, 1.0, 0.0, 1.0],
        intensity: 1.0,
        radius: 0.25,
        falloff: 2.0,
    };
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw_batch(
        &mut device,
        &box_program,
        &glowing.vertices_with(GuiShapeFill::Solid(glowing.fill), Some(&falloff_glow)),
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
    assert_eq!(grazing_ring.vertices().len(), 24);

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
                &probe.vertices(),
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
        &grazing_fill.vertices(),
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
        include_str!("../src/services/render/shaders/surface_gui.vert"),
        include_str!("../src/services/render/shaders/surface_gui.frag"),
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
    let tl = glyph_vertex([1.0, 1.0], [u0, v0], cyan);
    let bl = glyph_vertex([1.0, 2.0], [u0, v1], cyan);
    let br = glyph_vertex([2.0, 2.0], [u1, v1], cyan);
    let tr = glyph_vertex([2.0, 1.0], [u1, v0], cyan);
    let glyph_vertices = [tl, bl, br, tl, br, tr];
    let mut glyph_batch = upload(&mut device, &glyph_vertices, &ROOT)?;

    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    let atlas_tex = *ipp_render_gl::GlesRenderDevice::glyph_atlas_texture(&page);
    device.draw_gui_batch(&text_program, &glyph_batch, Some(&atlas_tex), &MVP, 0, 6)?;
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
    let tl_y = glyph_vertex([1.0, 1.0], [u0, v0], yellow);
    let bl_y = glyph_vertex([1.0, 2.0], [u0, v1], yellow);
    let br_y = glyph_vertex([2.0, 2.0], [u1, v1], yellow);
    let tr_y = glyph_vertex([2.0, 1.0], [u1, v0], yellow);
    let updated_vertices = [tl_y, bl_y, br_y, tl_y, br_y, tr_y];
    device.write_gui_batch(&mut glyph_batch, 0, &updated_vertices)?;

    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_gui_batch(&text_program, &glyph_batch, Some(&atlas_tex), &MVP, 0, 6)?;
    let updated_batch_frame = capture(&mut device, "glyph-batch-updated")?;
    check(
        &updated_batch_frame,
        120,
        120,
        [255, 255, 0, 255],
        8,
        "retained glyph batch updated yellow tint",
    )?;

    // One draw of boxes and glyphs whose vertices carry different clips: the box
    // keeps only its left part and the glyph quad only its lower half.
    let clipped_box = ProbeBox {
        placement: [2.25, 0.25, 1.5, 1.0],
        fill: [1.0, 0.0, 0.0, 1.0],
        border_color: [1.0; 4],
        corner: [0.0, 0.0],
        border: 0.0,
    };
    let box_clip = [2.25, 0.0, 3.0, 3.0];
    let glyph_clip = [0.0, 1.5, 4.0, 3.0];
    let mixed_vertices: Vec<_> = clipped_box
        .vertices()
        .into_iter()
        .map(|vertex| ipp_render_gl::GuiVertex {
            clip: box_clip,
            ..vertex
        })
        .chain(
            glyph_vertices
                .iter()
                .map(|vertex| ipp_render_gl::GuiVertex {
                    clip: glyph_clip,
                    ..*vertex
                }),
        )
        .collect();
    let mut mixed_batch = device.create_gui_batch(mixed_vertices.len())?;
    device.write_gui_batch(&mut mixed_batch, 0, &mixed_vertices)?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_gui_batch(
        &box_program,
        &mixed_batch,
        Some(&atlas_tex),
        &MVP,
        0,
        mixed_vertices.len(),
    )?;
    let mixed = capture(&mut device, "mixed-clips-one-draw")?;
    device.delete_gui_batch(mixed_batch);
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
    let outline_vertices = outline.vertices();
    assert_eq!(outline_vertices.len(), 24);

    let glyph_rectangle = [0.6, 1.8, 1.0, 0.8];
    let [left, top, right, bottom] = [
        glyph_rectangle[0],
        glyph_rectangle[1],
        glyph_rectangle[0] + glyph_rectangle[2],
        glyph_rectangle[1] + glyph_rectangle[3],
    ];
    let glyph_corner = |position: [f32; 2], uv: [f32; 2]| glyph_vertex(position, uv, cyan);
    let [glyph_tl, glyph_bl, glyph_br, glyph_tr] = [
        glyph_corner([left, top], [u0, v0]),
        glyph_corner([left, bottom], [u0, v1]),
        glyph_corner([right, bottom], [u1, v1]),
        glyph_corner([right, top], [u1, v0]),
    ];
    // Boxes and the glyph quad share one storage and draw as one range.
    let perspective_vertices = [
        filled.vertices(),
        outline_vertices,
        vec![glyph_tl, glyph_bl, glyph_br, glyph_tl, glyph_br, glyph_tr],
    ]
    .concat();
    let perspective_batch = upload(&mut device, &perspective_vertices, &ROOT)?;

    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_gui_batch(
        &box_program,
        &perspective_batch,
        Some(&atlas_tex),
        &oblique,
        0,
        perspective_vertices.len(),
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
            "nested=[{:?} {:?} {:?}]\ntext=[{:?} {:?}]\nbitmap=[{:?} {:?}]\ntilted_green={green_count}\nclose_border_profile={close_profile:?}\ngrazing=[{}]\nperspective_w=[{nearest:.3} {farthest:.3}]\nperspective_regions=[{}]\n{}\n",
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
            context.info().unwrap_or_else(|_| "no device info".into()),
        ),
    )?;
    println!(
        "PASS: nested clips, scrolled primitives, boxes, order, coverage ramps, glow, grazing and perspective views, Surface cache targets and recovery"
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The EGL GUI clip runner supports Linux; no graphics test was run.");
    std::process::exit(1);
}
