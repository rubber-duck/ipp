//! Maintained GLES driver for reusable custom-material frame assertions.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(dead_code)]
mod smoke;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: egl_custom_materials EGL_LIBRARY_DIR CUBE_MESH ARTIFACT_DIR".into());
    }
    let output = std::path::Path::new(&args[2]);
    std::fs::create_dir_all(output)?;
    let context = smoke::egl::Context::new(
        std::path::Path::new(&args[0]),
        smoke::world::WIDTH,
        smoke::world::HEIGHT,
    )?;
    std::fs::write(output.join("environment.txt"), context.info()?)?;
    let mut renderer = ipp_render_gl::RenderService::new(context.device()?)?;
    smoke::custom_materials::run(
        &mut renderer,
        &std::fs::read(&args[1])?,
        || context.capture(),
        || context.device(),
        output,
    )?;
    smoke::lighting::run(
        &mut renderer,
        &std::fs::read(&args[1])?,
        || context.capture(),
        output,
    )?;
    smoke::lighting::run_custom(
        &mut renderer,
        &std::fs::read(&args[1])?,
        || context.capture(),
        output,
    )?;
    println!(
        "PASS: custom shader parameters, independent layouts, program reuse and material fallback"
    );
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("This EGL environment driver requires Linux or macOS");
    std::process::exit(1);
}
