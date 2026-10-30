//! Maintained EGL runner for particle frame and lifetime assertions.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[allow(dead_code)]
mod smoke;

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: egl_particles EGL_LIBRARY_DIR ARTIFACT_DIR".into());
    }
    let output = std::path::Path::new(&args[1]);
    std::fs::create_dir_all(output)?;
    let context = smoke::egl::Context::new(
        std::path::Path::new(&args[0]),
        smoke::world::WIDTH,
        smoke::world::HEIGHT,
    )?;
    std::fs::write(output.join("environment.txt"), context.info()?)?;
    let mut renderer = ipp_render_gl::RenderService::new(context.device()?)?;
    smoke::particles::run(&mut renderer, || context.capture(), output)?;
    println!("PASS: instanced particles, private state preservation and lifetime drain");
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn main() {
    eprintln!("EGL particle runner requires Linux or macOS");
    std::process::exit(1);
}
