//! Linux EGL pbuffer smoke runner. Context/shim loading is test-host code only.

#[cfg(target_os = "linux")]
mod smoke;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use smoke::{egl, world};
    use smoke::{shapes, textures};
    use std::path::PathBuf;

    let mut args: Vec<_> = std::env::args_os().skip(1).collect();
    let lighting_only = args.first().is_some_and(|arg| arg == "--lighting-only");
    if lighting_only {
        args.remove(0);
    }
    let max_args = 5;
    if !(2..=max_args).contains(&args.len()) {
        return Err(
            "usage: egl_smoke [--lighting-only] <EGL-GLES-library-directory> <cube.mesh> [artifact-directory] [checker.texture] [shapes-directory]".into(),
        );
    }

    let library_dir = PathBuf::from(&args[0]);
    let fixture = std::fs::read(&args[1])?;
    let output = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/egl-smoke"));
    std::fs::create_dir_all(&output)?;
    // The context is declared first so all renderer resources are destroyed
    // before EGL teardown, on both normal return and error propagation.
    let context = egl::Context::new(&library_dir, world::WIDTH, world::HEIGHT)?;
    textures::validate_uploads(&mut context.device()?);
    let device = context.device()?;
    let mut renderer = ipp_render_gl::RenderService::new(device)?;
    let info = context.info()?;
    println!("{info}");
    std::fs::write(output.join("environment.txt"), &info)?;

    if lighting_only {
        smoke::lighting::run(&mut renderer, &fixture, || context.capture(), &output)?;
        println!("PASS: direct lights, soft spotlight shadows and material flags");
        return Ok(());
    }

    world::multiple_worlds(&mut renderer, &fixture, || context.capture(), &output)?;
    println!("PASS: shared resources, independent world frames and peer destruction");
    smoke::deferred_removal::run(&mut renderer, &fixture, || context.capture(), &output)?;
    println!(
        "PASS: deferred System removals retain update visibility and clear prepared GLES frames"
    );
    world::run(
        &mut renderer,
        fixture.clone(),
        || context.capture(),
        &output,
    )?;
    {
        smoke::lighting::run(&mut renderer, &fixture, || context.capture(), &output)?;
        println!("PASS: direct lights, spotlight shadows, material flags and resource recovery");
    }
    if let Some(texture_path) = args.get(3) {
        smoke::lighting::textures(
            &mut renderer,
            &fixture,
            &std::fs::read(texture_path)?,
            || context.capture(),
            &output,
        )?;
        println!("PASS: PBR texture modulation, light response and immutable resource recovery");
        textures::run(
            &mut renderer,
            fixture,
            std::fs::read(texture_path)?,
            || context.capture(),
            || context.device(),
            &output,
        )?;
        println!(
            "PASS: RGB checker, odd-width RGB/sRGB samples, invalid uploads, toggle, eviction, immutable-source recovery and partial-stream cancellation"
        );
    }
    if let Some(shapes_path) = args.get(4) {
        shapes::run(
            &mut renderer,
            &PathBuf::from(shapes_path),
            || context.capture(),
            &output,
        )?;
        println!("PASS: built-in solids, textured curves and clean debug contours");
        {
            smoke::lighting::normals(
                &mut renderer,
                &PathBuf::from(shapes_path),
                || context.capture(),
                &output,
            )?;
            println!("PASS: smooth normals, flat fallback, nonuniform scale and normal recovery");
        }
    }
    {
        smoke::debug_geometry::run(&mut renderer, || context.capture(), &output)?;
        println!(
            "PASS: debug solids/contours, uniform plane color, sparse global color and selection restoration"
        );
    }
    println!(
        "PASS: visible, moved, linear-to-sRGB, removed, cache reuse; artifacts {}",
        output.display()
    );
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The EGL smoke runner supports Linux; no graphics test was run.");
    std::process::exit(1);
}
