//! Native GLES host for the maintained mesh-pose endpoint fixtures.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod smoke;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: egl_mesh_poses <EGL-GLES-library-directory> <fixture-directory> <artifact-directory>".into());
    }
    let output = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&output)?;
    let context = smoke::egl::Context::new(
        &PathBuf::from(&args[0]),
        smoke::world::WIDTH,
        smoke::world::HEIGHT,
    )?;
    let mut renderer = ipp_render_gl::RenderService::new(context.device()?)?;
    std::fs::write(output.join("environment.txt"), context.info()?)?;
    smoke::mesh_poses::run(
        &mut renderer,
        &PathBuf::from(&args[1]),
        || context.capture(),
        || context.device(),
        &output,
    )?;
    println!("PASS: GLES mesh poses match baked geometry and recover shared buffers");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The EGL mesh-pose runner requires Linux");
    std::process::exit(1);
}
