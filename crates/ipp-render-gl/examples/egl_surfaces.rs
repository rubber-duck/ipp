//! Focused native GLES shader, curve coverage, Surface cache target and RGBA
//! Surface frame oracle.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod smoke;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use ipp_render_gl::RenderDevice;
    use std::path::PathBuf;

    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err("usage: egl_surfaces <EGL-GLES-library-directory> <surface-assets-directory> <font-assets-directory> <evidence-directory>".into());
    }
    let library_dir = PathBuf::from(&args[0]);
    let assets = PathBuf::from(&args[1]);
    let fonts = PathBuf::from(&args[2]);
    let evidence = PathBuf::from(&args[3]);
    std::fs::create_dir_all(&evidence)?;

    const WIDTH: u32 = smoke::world::WIDTH;
    const HEIGHT: u32 = smoke::world::HEIGHT;
    let context = smoke::egl::Context::new(&library_dir, WIDTH, HEIGHT)?;
    let mut device = context.device()?;
    let path_program = device.create_program(
        include_str!("../src/services/render/shaders/surface.vert"),
        include_str!("../src/services/render/shaders/surface.frag"),
    )?;
    let bitmap_program = device.create_program(
        include_str!("../src/services/render/shaders/surface_bitmap.vert"),
        include_str!("../src/services/render/shaders/surface_bitmap.frag"),
    )?;
    let bands = vec![[32, 8]; 32];
    let mut bands = bands;
    bands.extend((0..8).map(|curve| [curve, 0]));
    let path = device.create_surface_path(
        &[-0.65, -0.65, 0.65, 0.65],
        &[
            [0.65, 0.0, 0.65, 0.65, 0.0, 0.65, 1.0, 0.0],
            [0.0, 0.65, -0.65, 0.65, -0.65, 0.0, 1.0, 0.0],
            [-0.65, 0.0, -0.65, -0.65, 0.0, -0.65, 1.0, 0.0],
            [0.0, -0.65, 0.65, -0.65, 0.65, 0.0, 1.0, 0.0],
            [0.28, 0.0, 0.28, -0.28, 0.0, -0.28, 1.0, 0.0],
            [0.0, -0.28, -0.28, -0.28, -0.28, 0.0, 1.0, 0.0],
            [-0.28, 0.0, -0.28, 0.28, 0.0, 0.28, 1.0, 0.0],
            [0.0, 0.28, 0.28, 0.28, 0.28, 0.0, 1.0, 0.0],
        ],
        &bands,
    )?;
    let texture = device.create_texture(1, 1, &[0, 255, 0, 128])?;
    let identity = [
        1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let clip = [-1.0, -1.0, 1.0, 1.0];
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_path(
        &path_program,
        &path,
        &[-0.65, -0.65, 0.65, 0.65],
        ipp_render_gl::SurfacePathDescriptor::new([0, 8], 0),
        &identity,
        &[0.0, 0.0, 1.0, 1.0],
        &clip,
        &[1.0, 0.0, 0.0, 1.0],
        0,
    )?;
    device.draw_surface_bitmap(
        &bitmap_program,
        &texture,
        &identity,
        &[0.75, -0.07, 0.14, 0.14],
        &clip,
        &[1.0; 4],
    )?;
    device.end_frame()?;
    let pixels = context.capture()?;
    let center = &pixels[((HEIGHT / 2 * WIDTH + WIDTH / 2) * 4) as usize..][..4];
    if center != [0, 0, 0, 255] {
        return Err(format!("quadratic counter failed to preserve its hole: {center:?}").into());
    }
    let ring = &pixels[((HEIGHT / 2 * WIDTH + WIDTH * 11 / 16) * 4) as usize..][..4];
    if ring[0] < 220 || ring[1] > 30 || ring[3] != 255 {
        return Err(format!("quadratic ring coverage mismatch: {ring:?}").into());
    }
    let bitmap = &pixels[((HEIGHT / 2 * WIDTH + WIDTH * 9 / 10) * 4) as usize..][..4];
    if bitmap[1] <= bitmap[0] || bitmap[3] != 255 {
        return Err(format!("straight-alpha bitmap mismatch: {bitmap:?}").into());
    }
    std::fs::write(evidence.join("surface-frame.rgba"), &pixels)?;
    std::fs::write(
        evidence.join("surface-frame.txt"),
        format!("center={center:?}\nbitmap={bitmap:?}\n"),
    )?;
    let slash_bands = {
        let mut values = vec![[32, 4]; 32];
        values.extend((0..4).map(|curve| [curve, 0]));
        values
    };
    // Deliberately retain the old midpoint representation here. The kind lane
    // must force a stable direct line intersection even with raw font units.
    let slash = device.create_surface_path(
        &[82.0, -143.0, 518.0, 823.0],
        &[
            [82.0, -143.0, 257.0, 340.0, 432.0, 823.0, 0.0, 0.0],
            [432.0, 823.0, 475.0, 823.0, 518.0, 823.0, 0.0, 0.0],
            [518.0, 823.0, 343.0, 340.0, 168.0, -143.0, 0.0, 0.0],
            [168.0, -143.0, 125.0, -143.0, 82.0, -143.0, 0.0, 0.0],
        ],
        &slash_bands,
    )?;
    device.begin_frame(WIDTH, HEIGHT, &[0.0, 0.0, 0.0, 1.0])?;
    device.draw_surface_path(
        &path_program,
        &slash,
        &[82.0, -143.0, 518.0, 823.0],
        ipp_render_gl::SurfacePathDescriptor::new([0, 4], 0),
        &identity,
        &[-0.300, -0.340, 0.001, 0.001],
        &clip,
        &[1.0, 0.0, 0.0, 1.0],
        0,
    )?;
    device.end_frame()?;
    let slash_pixels = context.capture()?;
    let stable_line = &slash_pixels[((67 * WIDTH + 140) * 4) as usize..][..4];
    if stable_line[0] < 220 || stable_line[1] > 30 || stable_line[2] > 30 {
        return Err(format!(
            "raw-font-unit line intersection became discontinuous: {stable_line:?}"
        )
        .into());
    }
    std::fs::write(evidence.join("surface-line-regression.rgba"), &slash_pixels)?;
    smoke::surface_cache_target::run(&context, &mut device, &evidence)?;
    device.delete_surface_path(slash);
    device.delete_surface_path(path);
    device.delete_texture(texture);
    device.delete_program(path_program);
    device.delete_program(bitmap_program);
    let mut renderer = ipp_render_gl::RenderService::new(context.device()?)?;
    smoke::surfaces::run(
        &mut renderer,
        &assets,
        &fonts,
        || context.capture(),
        || context.device(),
        &evidence,
    )?;
    println!(
        "PASS: analytic shader plus Host/World providers, font, drawing and RGBA Surface frame"
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The EGL Surface runner supports Linux; no graphics test was run.");
    std::process::exit(1);
}
