//! Device-level GL error-check oracle shared by the native Surface runners.
//!
//! A GL error raised in the middle of a frame is not reported by the routine
//! draws or Surface cache composites that follow it outside exhaustive mode; a
//! sampled frame end reports it instead. A Surface cache repaint's end, not its
//! begin, rejects an image whose pass raised an error. In GUI builds, replacing
//! retained storage makes that frame's end check. Exhaustive mode reports the
//! error at the next routine call.

use ipp_render_gl::{GlesRenderDevice, RenderDevice, RenderError};

const WIDTH: u32 = super::world::WIDTH;
const HEIGHT: u32 = super::world::HEIGHT;
const CLEAR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
/// Enough frames for any sampling interval the device may choose.
const MAX_FRAMES: usize = 64;
/// Content [0, 2] x [0, 1] metres to clip space.
const CONTENT: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0,
];

type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;
type Routine<'a> = &'a dyn Fn(&mut GlesRenderDevice) -> Result<(), RenderError>;

fn invalid_enum(result: &Result<(), RenderError>) -> bool {
    match result {
        Err(RenderError::RenderDevice(message)) => message.contains("0x0500"),
        _ => false,
    }
}

/// Raise an error in the first frame just before `routine`, which must succeed,
/// then run frames until a frame end reports the error.
fn frames_until_reported(
    context: &super::egl::Context,
    device: &mut GlesRenderDevice,
    draw: Routine<'_>,
    routine: Routine<'_>,
) -> ProbeResult<usize> {
    for frame in 1..=MAX_FRAMES {
        device.begin_frame(WIDTH, HEIGHT, &CLEAR)?;
        draw(device)?;
        if frame == 1 {
            context.raise_gl_error()?;
            routine(device).map_err(|error| {
                format!("a routine call reported {error:?} outside exhaustive mode")
            })?;
        }

        let ended = device.end_frame();
        if invalid_enum(&ended) {
            return Ok(frame);
        }
        ended.map_err(|error| format!("frame {frame} ended with {error:?}"))?;
    }
    Err(format!("no frame end reported the error within {MAX_FRAMES} frames").into())
}

pub(crate) fn run(context: &super::egl::Context, evidence: &std::path::Path) -> ProbeResult<()> {
    // A fresh device starts its sampling schedule with a checked frame end.
    let mut device = context.device()?;
    let program = device.create_program(
        include_str!("../../src/services/render/shaders/surface_bitmap.vert"),
        include_str!("../../src/services/render/shaders/surface_bitmap.frag"),
    )?;
    let compositor = device.create_program(
        include_str!("../../src/services/render/shaders/surface_bitmap.vert"),
        include_str!("../../src/services/render/shaders/surface_cache.frag"),
    )?;
    let white = device.create_texture(1, 1, &[255; 4])?;
    let target = device.create_surface_cache_target(32, 16)?;
    let draw = |device: &mut GlesRenderDevice| {
        device.draw_surface_bitmap(
            &program,
            &white,
            &CONTENT,
            &[0.0, 0.0, 1.0, 0.5],
            &[0.0, 0.0, 2.0, 1.0],
            &[1.0, 1.0, 1.0, 1.0],
        )
    };
    let composite = |device: &mut GlesRenderDevice| {
        device.draw_surface_cache(&compositor, &target, &CONTENT, &[2.0, 1.0])
    };
    let mut report = String::new();

    device.begin_frame(WIDTH, HEIGHT, &CLEAR)?;
    draw(&mut device)?;
    device.end_frame()?;

    let reported = frames_until_reported(context, &mut device, &draw, &draw)?;
    report.push_str(&format!(
        "error raised before a draw reported by the frame {reported} end\n"
    ));

    // The end of a repaint checks the whole pass.
    context.raise_gl_error()?;
    device.begin_surface_cache_target(&target)?;
    draw(&mut device)?;
    let repainted = device.end_surface_cache_target();
    if !invalid_enum(&repainted) {
        return Err(format!("a failed repaint ended with {repainted:?}").into());
    }
    report.push_str("repaint end rejected the failed pass\n");

    device.begin_surface_cache_target(&target)?;
    device.end_surface_cache_target()?;
    let reported = frames_until_reported(context, &mut device, &draw, &composite)?;
    report.push_str(&format!(
        "error raised before a composite reported by the frame {reported} end\n"
    ));

    // Writing retained storage checks the same frame's end.
    #[cfg(feature = "gui")]
    {
        let vertex = ipp_render_gl::GuiVertex::EMPTY;
        let mut batch = device.create_gui_batch(6)?;
        device.begin_frame(WIDTH, HEIGHT, &CLEAR)?;
        context.raise_gl_error()?;
        device.write_gui_batch(&mut batch, 0, &[vertex; 6])?;
        draw(&mut device)?;
        let ended = device.end_frame();
        device.delete_gui_batch(batch);
        if !invalid_enum(&ended) {
            return Err(format!("a retained replacement frame ended with {ended:?}").into());
        }
        report.push_str("retained replacement frame end reported the error\n");
    }

    // Exhaustive mode attributes the error to the next routine call.
    device.set_exhaustive_draw_checks(true);
    device.begin_frame(WIDTH, HEIGHT, &CLEAR)?;
    draw(&mut device)?;
    context.raise_gl_error()?;
    let attributed = draw(&mut device);
    device.end_frame()?;
    device.set_exhaustive_draw_checks(false);
    if !invalid_enum(&attributed) {
        return Err(format!("exhaustive mode reported {attributed:?} at the next draw").into());
    }
    report.push_str("exhaustive mode reported the error at the next draw\n");

    device.delete_surface_cache_target(target);
    device.delete_texture(white);
    device.delete_program(program);
    device.delete_program(compositor);
    std::fs::write(evidence.join("error-checks.txt"), report)?;
    Ok(())
}
