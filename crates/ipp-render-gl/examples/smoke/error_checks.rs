//! Device-level GL error-check oracle shared by the native Surface runners.
//!
//! A GL error raised in the middle of a frame is not reported by the routine
//! draws that follow it outside exhaustive mode; a sampled frame end reports
//! it instead. In GUI builds, replacing retained storage makes that frame's end
//! check. Exhaustive mode reports the error at the next routine call.

pub(crate) fn run(
    context: &super::egl::Context,
    evidence: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    use ipp_render_gl::{GlesRenderDevice, RenderDevice, RenderError};

    const WIDTH: u32 = super::world::WIDTH;
    const HEIGHT: u32 = super::world::HEIGHT;
    /// Enough frames for any sampling interval the device may choose.
    const MAX_FRAMES: usize = 64;

    // A fresh device starts its sampling schedule with a checked frame end.
    let mut device = context.device()?;
    let program = device.create_program(
        include_str!("../../src/services/render/shaders/surface_bitmap.vert"),
        include_str!("../../src/services/render/shaders/surface_bitmap.frag"),
    )?;
    let white = device.create_texture(1, 1, &[255; 4])?;
    let content = [
        1.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, -1.0, 0.0, 1.0,
    ];
    let draw = |device: &mut GlesRenderDevice| {
        device.draw_surface_bitmap(
            &program,
            &white,
            &content,
            &[0.0, 0.0, 1.0, 0.5],
            &[0.0, 0.0, 2.0, 1.0],
            &[1.0, 1.0, 1.0, 1.0],
        )
    };
    let invalid_enum = |result: &Result<(), RenderError>| match result {
        Err(RenderError::RenderDevice(message)) => message.contains("0x0500"),
        _ => false,
    };
    let mut report = String::new();

    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw(&mut device)?;
    device.end_frame()?;

    // Raised in the middle of the first sampled-out frame: the following draw
    // and unchecked frame ends succeed, and a later sampled frame end reports it.
    let mut reported = None;
    for frame in 1..=MAX_FRAMES {
        device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
        draw(&mut device)?;
        if frame == 1 {
            context.raise_gl_error()?;
        }
        draw(&mut device).map_err(|error| {
            format!("frame {frame}: a routine draw reported {error:?} outside exhaustive mode")
        })?;
        let ended = device.end_frame();
        if invalid_enum(&ended) {
            reported = Some(frame);
            break;
        }
        ended.map_err(|error| format!("frame {frame} ended with {error:?}"))?;
    }
    let Some(reported) = reported else {
        return Err(format!("no frame end reported the error within {MAX_FRAMES} frames").into());
    };
    report.push_str(&format!(
        "error raised in frame 1 reported by the frame {reported} end\n"
    ));

    // Replacing retained storage checks the same frame's end.
    #[cfg(feature = "gui")]
    {
        let vertex = ipp_render_gl::GuiBoxVertex {
            position: [0.0; 2],
            placement: [0.0; 4],
            shape: [0.0; 4],
            color0: [0.0; 4],
            color1: [0.0; 4],
            border_color: [0.0; 4],
            gradient_coords: [0.0; 4],
            material_params: [0.0; 4],
            glow_color: [0.0; 4],
        };
        let mut batch = device.create_gui_batch(&[vertex; 3])?;
        device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
        context.raise_gl_error()?;
        device.update_gui_batch(&mut batch, &[vertex; 6])?;
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
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    draw(&mut device)?;
    context.raise_gl_error()?;
    let attributed = draw(&mut device);
    device.end_frame()?;
    device.set_exhaustive_draw_checks(false);
    if !invalid_enum(&attributed) {
        return Err(format!("exhaustive mode reported {attributed:?} at the next draw").into());
    }
    report.push_str("exhaustive mode reported the error at the next draw\n");

    device.delete_texture(white);
    device.delete_program(program);
    std::fs::write(evidence.join("error-checks.txt"), report)?;
    Ok(())
}
