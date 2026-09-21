//! Focused native GLES clip and GUI box frame oracle.
//!
//! Device-level counterpart to the raw Surface scenes in `egl_surfaces`: nested
//! clip intersections, scrolled curves/text-proxies/bitmaps, parameterized
//! rounded boxes with explicit per-axis corner/border dimensions, empty-clip
//! suppression, painter-order overlap, a tilted view and device replacement.
//! All fixtures are synthetic and local; assertions compare completed-frame
//! pixels against values derived independently from the scene layout below.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod smoke;

/// Surface content space: 4 x 3 metres at 80 pixels per metre on 320 x 240.
/// Content (x, y) in top-left/Y-down metres lands on pixel (80x, 80y).
#[cfg(target_os = "linux")]
const ROOT: [f32; 4] = [0.0, 0.0, 4.0, 3.0];

#[cfg(target_os = "linux")]
const MVP: [f32; 16] = [
    0.5, 0.0, 0.0, 0.0, 0.0, -0.6666667, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 1.0, 0.0, 1.0,
];

/// The filled/rounded/bordered reference frame, shared by the initial capture
/// and the device-replacement recovery check.
#[cfg(target_os = "linux")]
fn draw_boxes<D: ipp_render_gl::RenderDevice>(
    device: &mut D,
    box_program: &D::Program,
) -> Result<(), Box<dyn std::error::Error>> {
    use ipp_render_gl::SurfaceBoxShape;

    device.begin_frame(
        smoke::world::WIDTH,
        smoke::world::HEIGHT,
        &[0.0, 0.0, 0.0, 1.0],
    )?;
    device.draw_surface_box(
        box_program,
        &MVP,
        &[0.5, 0.5, 1.0, 1.0],
        &ROOT,
        &[1.0, 0.0, 0.0, 1.0],
        &[1.0, 1.0, 1.0, 1.0],
        SurfaceBoxShape {
            corner: [0.0, 0.0],
            border: 0.0,
        },
    )?;
    device.draw_surface_box(
        box_program,
        &MVP,
        &[2.0, 0.5, 1.0, 1.0],
        &ROOT,
        &[0.0, 1.0, 0.0, 1.0],
        &[1.0, 1.0, 1.0, 1.0],
        SurfaceBoxShape {
            corner: [0.35, 0.12],
            border: 0.0,
        },
    )?;
    device.draw_surface_box(
        box_program,
        &MVP,
        &[0.5, 1.75, 1.0, 0.75],
        &ROOT,
        &[0.0, 0.0, 1.0, 1.0],
        &[1.0, 1.0, 1.0, 1.0],
        SurfaceBoxShape {
            corner: [0.1, 0.1],
            border: 0.05,
        },
    )?;
    device.draw_surface_box(
        box_program,
        &MVP,
        &[2.0, 1.75, 1.0, 0.75],
        &ROOT,
        &[1.0, 1.0, 0.0, 1.0],
        &[1.0, 1.0, 1.0, 1.0],
        SurfaceBoxShape {
            corner: [0.0, 0.2],
            border: 0.0,
        },
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use ipp_render_gl::{RenderDevice, SurfaceBoxShape, SurfacePathDescriptor};
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
        include_str!("../src/services/render/shaders/surface_box.vert"),
        include_str!("../src/services/render/shaders/surface_box.frag"),
    )?;

    // Unit-square contour used as scrolled curves, text-proxy glyphs and
    // painter-order probes. Headers reference every curve, as in egl_surfaces.
    let mut bands = vec![[32, 4]; 32];
    bands.extend((0..4).map(|curve| [curve, 0]));
    let square = device.create_surface_path(
        &[0.0, 0.0, 1.0, 1.0],
        &[
            [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0],
            [1.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ],
        &bands,
    )?;
    let square_descriptor = SurfacePathDescriptor::new([0, 4], 0);
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
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_path_instances(
        &instance_program,
        &square,
        &glyphs(0.0),
        &MVP,
        &text_clip,
        0,
    )?;
    let text_before = capture(&mut device, "clip-text")?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_path_instances(
        &instance_program,
        &square,
        &glyphs(1.0),
        &MVP,
        &text_clip,
        0,
    )?;
    let text_after = capture(&mut device, "clip-text-scrolled")?;
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
    device.draw_surface_box(
        &box_program,
        &MVP,
        &[0.5, 0.5, 1.0, 1.0],
        &empty,
        &[1.0, 0.0, 0.0, 1.0],
        &[1.0, 1.0, 1.0, 1.0],
        SurfaceBoxShape {
            corner: [0.1, 0.1],
            border: 0.02,
        },
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
        device.draw_surface_box(
            &box_program,
            &MVP,
            &placement,
            &ROOT,
            &[1.0, 0.0, 0.0, 1.0],
            &[1.0, 1.0, 1.0, 1.0],
            SurfaceBoxShape {
                corner: [0.15, 0.15],
                border: 0.03,
            },
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
    let red: ([f32; 4], [f32; 4], SurfaceBoxShape) = (
        [1.0, 1.0, 1.5, 1.0],
        [1.0, 0.0, 0.0, 1.0],
        SurfaceBoxShape {
            corner: [0.0, 0.0],
            border: 0.0,
        },
    );
    let green: ([f32; 4], [f32; 4], SurfaceBoxShape) = (
        [1.75, 1.25, 1.5, 1.0],
        [0.0, 1.0, 0.0, 1.0],
        SurfaceBoxShape {
            corner: [0.2, 0.2],
            border: 0.0,
        },
    );
    let paint = |device: &mut ipp_render_gl::GlesRenderDevice,
                 order: &[&([f32; 4], [f32; 4], SurfaceBoxShape)]| {
        device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
        for entry in order {
            device.draw_surface_box(
                &box_program,
                &MVP,
                &entry.0,
                &ROOT,
                &entry.1,
                &[0.0, 0.0, 0.0, 1.0],
                entry.2,
            )?;
        }
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    paint(&mut device, &[&red, &green])?;
    let green_over = capture(&mut device, "boxes-green-over-red")?;
    paint(&mut device, &[&green, &red])?;
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
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_box(
        &box_program,
        &tilted,
        &[1.0, 0.75, 2.0, 1.5],
        &ROOT,
        &[0.0, 1.0, 0.0, 1.0],
        &[1.0, 1.0, 1.0, 1.0],
        SurfaceBoxShape {
            corner: [0.2, 0.2],
            border: 0.0,
        },
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
        include_str!("../src/services/render/shaders/surface_box.vert"),
        include_str!("../src/services/render/shaders/surface_box.frag"),
    )?;
    draw_boxes(&mut device, &box_program)?;
    let recovered = capture(&mut device, "boxes-recovered")?;
    if recovered != boxes {
        return Err("recreated device must redraw identical boxes".into());
    }
    device.delete_program(box_program);

    std::fs::write(
        evidence.join("gui-clips.txt"),
        format!(
            "nested=[{:?} {:?} {:?}]\ntext=[{:?} {:?}]\nbitmap=[{:?} {:?}]\ntilted_green={green_count}\n{}\n",
            pixel(&nested, 120, 120),
            pixel(&nested, 60, 60),
            pixel(&nested, 240, 40),
            pixel(&text_before, 80, 92),
            pixel(&text_after, 80, 92),
            pixel(&bitmap_before, 120, 120),
            pixel(&bitmap_after, 120, 120),
            context.info().unwrap_or_else(|_| "no device info".into()),
        ),
    )?;
    println!("PASS: nested clips, scrolled primitives, boxes, order, tilt and recovery");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The EGL GUI clip runner supports Linux; no graphics test was run.");
    std::process::exit(1);
}
