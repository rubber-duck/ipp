//! Real GLES skinning runner with reusable fixture data and host-owned frame capture.

#[cfg(target_os = "linux")]
#[allow(dead_code)]
mod smoke;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: egl_skinning <EGL-GLES-library-directory> <rig-fixture-directory> <artifact-directory>".into());
    }
    let output = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&output)?;
    let context = smoke::egl::Context::new(
        &PathBuf::from(&args[0]),
        smoke::world::WIDTH,
        smoke::world::HEIGHT,
    )?;
    let mut renderer = ipp_render_gl::RenderService::new(context.device()?)?;
    let info = context.info()?;
    println!("{info}");
    std::fs::write(output.join("environment.txt"), info)?;
    smoke::skinning::run(
        &mut renderer,
        &PathBuf::from(&args[1]),
        || context.capture(),
        || context.device(),
        &output,
    )?;
    println!(
        "PASS: GLES rest/bent poses, independent instances, four influences, and context resource recovery"
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The EGL skinning runner requires Linux");
    std::process::exit(1);
}
