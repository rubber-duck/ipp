//! Completed-publication scenarios using the maintained EGL capture driver.

#[cfg(target_os = "linux")]
mod smoke {
    pub mod canvas_assets;
    pub mod canvas_publications;
    pub mod composed_queries;
    pub mod egl;
    pub mod gui_cache_interaction;
    pub mod gui_control_publications;
    pub mod gui_publications;
    pub mod publications;
    #[allow(dead_code)]
    pub mod selection;
    pub mod surface_visibility;
}

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::PathBuf;
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 4 {
        return Err("usage: egl_publications <EGL-library-directory> <artifact-directory> <surface-assets> <font-assets>".into());
    }
    let output = PathBuf::from(&arguments[1]);
    std::fs::create_dir_all(&output)?;
    let context = smoke::egl::Context::new(&PathBuf::from(&arguments[0]), 256, 256)?;
    std::fs::write(output.join("environment.txt"), context.info()?)?;
    let mut renderer = ipp_render_gl::RenderService::new(context.device()?)?;
    smoke::composed_queries::run(&mut renderer, || context.capture(), &output)?;
    smoke::publications::run(
        &mut renderer,
        || context.capture(),
        || context.device(),
        &output,
    )?;
    smoke::canvas_publications::run(&mut renderer, || context.capture(), &output)?;
    smoke::canvas_assets::run(
        &mut renderer,
        || context.capture(),
        &output,
        &PathBuf::from(&arguments[2]),
        &PathBuf::from(&arguments[3]),
    )?;
    smoke::surface_visibility::run(
        &mut renderer,
        || context.capture(),
        &output,
        &PathBuf::from(&arguments[3]),
    )?;
    smoke::gui_publications::run(
        &mut renderer,
        || context.capture(),
        &output,
        &PathBuf::from(&arguments[3]),
    )?;
    smoke::gui_cache_interaction::run(
        &mut renderer,
        || context.capture(),
        || context.device(),
        &output,
    )?;
    smoke::gui_control_publications::run(
        &mut renderer,
        || context.capture(),
        &output,
        &PathBuf::from(&arguments[3]),
    )?;
    println!("PASS: publication-backed rendering scenarios");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("EGL publication capture requires Linux");
}
