use super::{HEIGHT, Renderer, Result, WIDTH, fixture::Scene};
use ipp_core::systems::animation::AnimationPlaybackControl;
use ipp_render_gl::RenderStats;
use std::{io::Write, path::Path, time::Instant};

#[derive(Clone, Copy)]
pub(super) struct Frame {
    update: f64,
    render: f64,
    completion: f64,
    total: f64,
    stats: RenderStats,
}

pub(super) fn frame(
    scene: &mut Scene,
    renderer: &mut Renderer,
    context: &crate::egl::Context,
    dt: f64,
    render: bool,
) -> Result<Frame> {
    let started = Instant::now();
    if render {
        renderer.begin_frame();
    }
    scene.update(dt)?;
    let update = started.elapsed().as_secs_f64() * 1000.0;
    let submitted = Instant::now();
    let stats = if render {
        renderer.render(
            &mut scene.host.world_mut(scene.world).unwrap(),
            WIDTH,
            HEIGHT,
        )?
    } else {
        RenderStats::default()
    };
    let render_ms = submitted.elapsed().as_secs_f64() * 1000.0;
    let waiting = Instant::now();
    if render {
        context.finish()?;
    }
    let completion = waiting.elapsed().as_secs_f64() * 1000.0;
    Ok(Frame {
        update,
        render: render_ms,
        completion,
        total: started.elapsed().as_secs_f64() * 1000.0,
        stats,
    })
}

pub(super) fn validate(stats: RenderStats) -> Result<()> {
    if stats.failed_draw_calls != 0 || stats.invalid_camera {
        return Err(format!("invalid native frame: {stats:?}").into());
    }
    Ok(())
}

pub(super) fn measure(
    label: &str,
    scene: &mut Scene,
    renderer: &mut Renderer,
    context: &crate::egl::Context,
    step: (f64, bool),
    count: usize,
    output: &Path,
) -> Result<()> {
    let (dt, render) = step;
    for _ in 0..5 {
        frame(scene, renderer, context, 0.0, render)?;
    }
    let mut frames = Vec::with_capacity(count);
    for _ in 0..count {
        let sample = frame(scene, renderer, context, dt, render)?;
        validate(sample.stats)?;
        frames.push(sample);
    }
    #[cfg(feature = "profiling")]
    {
        use ipp_core::profiling as profile;
        profile::reset(true);
        let result = (|| {
            for _ in 0..5 {
                validate(frame(scene, renderer, context, dt, render)?.stats)?;
            }
            Ok::<_, Box<dyn std::error::Error>>(())
        })();
        profile::pause();
        result?;
        let allocations = profile::allocations();
        println!(
            "{label}: Rust allocations/frame {}, requested bytes/frame {}",
            allocations.0 as f64 / 5.0,
            allocations.1 as f64 / 5.0
        );
        std::fs::write(
            output.join(format!("{label}-allocations.json")),
            format!(
                "{{\"frames\":5,\"dt\":{dt},\"calls\":{},\"requested_bytes\":{}}}\n",
                allocations.0, allocations.1
            ),
        )?;
        let mut stages = std::fs::File::create(output.join(format!("{label}-stages.csv")))?;
        writeln!(
            stages,
            "system,phase,calls,ms_per_frame,allocations,requested_bytes"
        )?;
        for system in 0..32 {
            let name = profile::system_name(system);
            if name.is_empty() {
                continue;
            }
            for (phase, label) in [
                "check", "accept", "restore", "prepare", "evaluate", "finish",
            ]
            .iter()
            .enumerate()
            {
                let offset = (system * 6 + phase) * 4;
                if profile::counter(offset) == 0 {
                    continue;
                }
                let label = if name.starts_with("profile.") {
                    &"scope"
                } else {
                    label
                };
                writeln!(
                    stages,
                    "{name},{label},{},{},{},{}",
                    profile::counter(offset),
                    profile::counter(offset + 1) as f64 / 5e6,
                    profile::counter(offset + 2),
                    profile::counter(offset + 3)
                )?;
            }
        }
        if allocations != (0, 0) {
            return Err(format!(
                "{label}: warmed Rust allocation budget exceeded: {allocations:?}"
            )
            .into());
        }
    }
    let mut csv = std::fs::File::create(output.join(format!("{label}.csv")))?;
    writeln!(
        csv,
        "frame,update_ms,render_cpu_ms,gpu_completion_wait_ms,total_ms,draws,shadow_draws,triangles"
    )?;
    for (index, frame) in frames.iter().enumerate() {
        writeln!(
            csv,
            "{index},{},{},{},{},{},{},{}",
            frame.update,
            frame.render,
            frame.completion,
            frame.total,
            frame.stats.draw_calls,
            frame.stats.shadow_draw_calls,
            frame.stats.triangles
        )?;
    }
    let percentile = |value: fn(&Frame) -> f64, fraction: f64| {
        let mut values: Vec<_> = frames.iter().map(value).collect();
        values.sort_unstable_by(f64::total_cmp);
        values[((values.len() - 1) as f64 * fraction).ceil() as usize]
    };
    println!(
        "{label}: median update {:.3} ms; render CPU {:.3} ms; completion wait {:.3} ms; total {:.3} ms; p95 total {:.3} ms; draws {}/{}",
        percentile(|f| f.update, 0.5),
        percentile(|f| f.render, 0.5),
        percentile(|f| f.completion, 0.5),
        percentile(|f| f.total, 0.5),
        percentile(|f| f.total, 0.95),
        frames[0].stats.draw_calls,
        frames[0].stats.shadow_draw_calls
    );
    Ok(())
}

pub(super) fn capture(context: &crate::egl::Context, output: &Path, name: &str) -> Result<Vec<u8>> {
    let pixels = context.capture()?;
    let mut file =
        std::io::BufWriter::new(std::fs::File::create(output.join(format!("{name}.ppm")))?);
    write!(file, "P6\n{WIDTH} {HEIGHT}\n255\n")?;
    for pixel in pixels.as_chunks::<4>().0 {
        file.write_all(&pixel[..3])?;
    }
    let changed = pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[..3] != [10, 14, 20])
        .count();
    if changed < 100 {
        return Err("native benchmark frame is empty".into());
    }
    Ok(pixels)
}

pub(crate) fn run() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() < 4
        || args[4..].iter().any(|arg| {
            ![
                "--allow-software",
                "--skip-moving",
                "--mixed",
                "--draw-sweep",
                "--culling-views",
                "--render-profile",
                "--geometry-index=flat",
                "--geometry-index=bvh",
            ]
            .iter()
            .any(|flag| arg == flag)
        })
    {
        return Err(
            "usage: profile_scene EGL_DIR NATIVE_BUNDLE OUTPUT_DIR FRAMES [--allow-software] [--skip-moving]"
                .into(),
        );
    }
    if [
        "--culling-views",
        "--draw-sweep",
        "--render-profile",
        "--mixed",
    ]
    .iter()
    .filter(|flag| args[4..].iter().any(|arg| arg == *flag))
    .count()
        > 1
    {
        return Err("select only one native benchmark scenario".into());
    }
    let output = Path::new(&args[2]);
    std::fs::create_dir_all(output)?;
    let count: usize = args[3].to_str().ok_or("invalid frame count")?.parse()?;
    if count == 0 {
        return Err("frame count must be positive".into());
    }
    // Context is dropped after the World/provider graph and renderer.
    let context = crate::egl::Context::new(Path::new(&args[0]), WIDTH, HEIGHT)?;
    let info = context.info()?;
    println!("{info}");
    std::fs::write(output.join("environment.txt"), &info)?;
    let software = ["llvmpipe", "softpipe", "swiftshader", "software"]
        .iter()
        .any(|name| info.to_ascii_lowercase().contains(name));
    if software && !args[4..].iter().any(|arg| arg == "--allow-software") {
        return Err("hardware GLES required; refusing software renderer".into());
    }
    let draw_sweep = args[4..].iter().any(|arg| arg == "--draw-sweep");
    let device = if draw_sweep {
        super::draw_probe::device(&context)?
    } else {
        context.device()?
    };
    let mut renderer = Renderer::new(device)?;
    let mut scene = Scene::load(Path::new(&args[1]), &renderer)?;
    if args[4..].iter().any(|arg| arg == "--geometry-index=flat") {
        scene
            .host
            .world_mut(scene.world)
            .unwrap()
            .set_geometry_spatial_backend(
                ipp_core::systems::geometry::GeometrySpatialBackend::Flat,
            );
    }
    // Shader readiness is driven by ordinary renderer demand and Host service progression.
    for _ in 0..20 {
        frame(&mut scene, &mut renderer, &context, 0.0, true)?;
    }
    validate(frame(&mut scene, &mut renderer, &context, 0.0, true)?.stats)?;
    let before = capture(&context, output, "held-default-bounds")?;
    if args[4..].iter().any(|arg| arg == "--culling-views") {
        super::culling_views::run(
            &mut scene,
            &mut renderer,
            &context,
            count,
            output,
            Path::new(&args[1]),
        )?;
        scene.validate_probes(&output.join("probes.tsv"))?;
        renderer.forget_world(scene.world);
        if !scene.host.destroy_world(scene.world) {
            return Err("culling comparison World teardown failed".into());
        }
        println!(
            "PASS: interior/overview cameras, matching BVH/flat visibility and captures, animation probes and teardown"
        );
        return Ok(());
    }
    if draw_sweep {
        super::draw_sweep::run(&mut scene, &mut renderer, &context, count, output)?;
        scene.validate_probes(&output.join("probes.tsv"))?;
        renderer.forget_world(scene.world);
        if !scene.host.destroy_world(scene.world) {
            return Err("draw sweep World teardown failed".into());
        }
        println!("PASS: native draw attribution, restored captures, Blender probes and teardown");
        return Ok(());
    }
    if args[4..].iter().any(|arg| arg == "--render-profile") {
        super::draw_sweep::profile(&mut scene, &mut renderer, &context, count, output)?;
        scene.validate_probes(&output.join("probes.tsv"))?;
        renderer.forget_world(scene.world);
        if !scene.host.destroy_world(scene.world) {
            return Err("render profile World teardown failed".into());
        }
        println!("PASS: native renderer sampling window, captures, Blender probes and teardown");
        return Ok(());
    }
    if args[4..].iter().any(|arg| arg == "--mixed") {
        measure(
            "mixed-held-render",
            &mut scene,
            &mut renderer,
            &context,
            (0.0, true),
            count,
            output,
        )?;
        measure(
            "mixed-held-update",
            &mut scene,
            &mut renderer,
            &context,
            (0.0, false),
            count,
            output,
        )?;
        scene.control(AnimationPlaybackControl::Play)?;
        measure(
            "mixed-moving-update",
            &mut scene,
            &mut renderer,
            &context,
            (1.0 / 120.0, false),
            count,
            output,
        )?;
        scene.control(AnimationPlaybackControl::Pause)?;
        scene.control(AnimationPlaybackControl::Seek(6.0))?;
        for _ in 0..10 {
            frame(&mut scene, &mut renderer, &context, 0.0, true)?;
        }
        let switched = capture(&context, output, "mixed-switched")?;
        if switched == before {
            return Err("mixed resource transition did not change pixels".into());
        }
        measure(
            "mixed-switched-update",
            &mut scene,
            &mut renderer,
            &context,
            (0.0, false),
            count,
            output,
        )?;
        scene.control(AnimationPlaybackControl::Stop)?;
        for _ in 0..10 {
            frame(&mut scene, &mut renderer, &context, 0.0, true)?;
        }
        let stopped = capture(&context, output, "mixed-stopped")?;
        if stopped == switched || stopped == before {
            return Err("mixed stop did not restore pixels".into());
        }
        renderer.forget_world(scene.world);
        if !scene.host.destroy_world(scene.world) {
            return Err("mixed World teardown failed".into());
        }
        println!(
            "PASS: mixed native WebSocket-authored World, numeric/resource animation, GLES transitions and stop restoration"
        );
        return Ok(());
    }
    #[cfg(feature = "profiling")]
    {
        let mut world = scene.host.world_mut(scene.world).unwrap();
        let started = Instant::now();
        let (drivers, copied_bytes, segment_bytes) = world.profile_rebind_animation_tracks()?;
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "Prepared {drivers} typed driver tracks in {elapsed:.3} ms; copied track data {copied_bytes} bytes; inline working segments {segment_bytes} bytes"
        );
        std::fs::write(
            output.join("track-storage.json"),
            format!(
                "{{\"drivers\":{drivers},\"copied_track_bytes\":{copied_bytes},\"rebind_ms\":{elapsed},\"inline_segment_bytes\":{segment_bytes}}}\n"
            ),
        )?;
        // The standalone typed-curve comparison only defines a unit-weight,
        // fixed-property cost floor. Mixed operators still run in every frame
        // measurement below; excluding this microbenchmark never removes drivers.
        let mixed_operators = world.animation_controllers().iter().any(|controller| {
            controller.description.drivers.iter().any(|driver| {
                driver.additive
                    || driver.weight != 1.0
                    || matches!(
                        driver.property,
                        ipp_core::systems::animation::AnimationTrackTarget::DynamicProperty { .. }
                    )
            })
        });
        if mixed_operators {
            println!(
                "Curve-only comparison omitted: fixture includes weighted, additive or dynamic drivers; whole-frame measurements retain all drivers"
            );
        } else {
            // Seek between the 24 Hz baked keys to include interpolation math.
            let offset = 1.0 / 120.0;
            let resolved = world.profile_resolved_curve_sampler(offset)?;
            let expected = world.profile_animation_curve_samples(offset)?;
            if resolved() != expected {
                return Err("curve probe sample counts differ".into());
            }
            let mut csv = std::fs::File::create(output.join("curve-sampling-only.csv"))?;
            writeln!(csv, "mode,frame,samples,time_offset,ms")?;
            for (block, direct) in [false, true, true, false].into_iter().enumerate() {
                let sample = || -> Result<usize> {
                    if direct {
                        Ok(resolved())
                    } else {
                        Ok(world.profile_animation_curve_samples(offset)?)
                    }
                };
                for _ in 0..5 {
                    sample()?;
                }
                let mut timings = Vec::with_capacity(count);
                for _ in 0..count {
                    let start = Instant::now();
                    std::hint::black_box(sample()?);
                    timings.push(start.elapsed().as_secs_f64() * 1000.0);
                }
                let mode = if direct {
                    "resolved"
                } else {
                    "driver"
                };
                for (index, ms) in timings.iter().enumerate() {
                    writeln!(csv, "{mode}-{block},{index},{expected},{offset},{ms}")?;
                }
                timings.sort_unstable_by(f64::total_cmp);
                println!(
                    "Curve sampling {mode}-{block}: {expected} actual bound tracks, median {:.3} ms (no component staging/commits; joint poses excluded)",
                    timings[count / 2]
                );
            }
        }
    }
    measure(
        "held-update-only",
        &mut scene,
        &mut renderer,
        &context,
        (0.0, false),
        count,
        output,
    )?;
    measure(
        "held-default-bounds",
        &mut scene,
        &mut renderer,
        &context,
        (0.0, true),
        count,
        output,
    )?;
    scene.use_authored_bounds()?;
    for _ in 0..5 {
        frame(&mut scene, &mut renderer, &context, 0.0, true)?;
    }
    let after = capture(&context, output, "held-culled")?;
    if before != after {
        return Err("authored generated bounds changed held pixels".into());
    }
    measure(
        "held-culled",
        &mut scene,
        &mut renderer,
        &context,
        (0.0, true),
        count,
        output,
    )?;
    if !args[4..].iter().any(|arg| arg == "--skip-moving") {
        scene.control(AnimationPlaybackControl::Seek(0.0))?;
        scene.control(AnimationPlaybackControl::Play)?;
        // The benchmark Host supplies a deterministic replay clock. No client step API.
        for _ in 0..200 {
            frame(&mut scene, &mut renderer, &context, 1.0 / 60.0, true)?;
        }
        measure(
            "moving-culled",
            &mut scene,
            &mut renderer,
            &context,
            (1.0 / 60.0, true),
            count,
            output,
        )?;
        let moved = capture(&context, output, "moving")?;
        if moved == before {
            return Err("native animation did not change pixels".into());
        }
        measure(
            "moving-update-only",
            &mut scene,
            &mut renderer,
            &context,
            (1.0 / 60.0, false),
            count,
            output,
        )?;
        scene.control(AnimationPlaybackControl::Pause)?;
        measure(
            "moving-pose-update-only",
            &mut scene,
            &mut renderer,
            &context,
            (0.0, false),
            count,
            output,
        )?;
    }
    scene.validate_probes(&output.join("probes.tsv"))?;
    renderer.forget_world(scene.world);
    if !scene.host.destroy_world(scene.world) {
        return Err("native World teardown failed".into());
    }
    println!(
        "PASS: native saved World, assets, animated rendering, equivalent automatic/authored bounds and clean teardown"
    );
    Ok(())
}
