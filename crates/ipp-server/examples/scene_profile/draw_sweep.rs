//! Controlled submissions of identical evaluated scenes through real native GLES.
use super::{HEIGHT, Renderer, Result, WIDTH, draw_probe as probe, fixture::Scene, measurement};
use ipp_core::systems::animation::AnimationPlaybackControl;
use ipp_render_gl::RenderStats;
use std::{io::Write, path::Path, time::Instant};

struct Sample {
    render_ms: f64,
    completion_ms: f64,
    attempted: u64,
    submitted: u64,
    stats: RenderStats,
    metrics: [probe::Metric; 48],
}

fn render(
    scene: &mut Scene,
    renderer: &mut Renderer,
    context: &crate::egl::Context,
    mode: probe::Mode,
    timing: bool,
) -> Result<Sample> {
    probe::start(mode, timing);
    let started = Instant::now();
    let result = renderer.render(
        &mut scene.host.world_mut(scene.world).unwrap(),
        WIDTH,
        HEIGHT,
    );
    let render_ms = started.elapsed().as_secs_f64() * 1000.0;
    let (attempted, submitted, metrics) = probe::stop();
    let waiting = Instant::now();
    context.finish()?;
    let completion_ms = waiting.elapsed().as_secs_f64() * 1000.0;
    let stats = result?;
    measurement::validate(stats)?;
    if attempted != u64::from(stats.draw_calls + stats.shadow_draw_calls) {
        return Err(format!("draw interception count differs: {attempted}, {stats:?}").into());
    }
    let expected = match mode {
        probe::Mode::Normal | probe::Mode::Discard => attempted,
        probe::Mode::NoDraws | probe::Mode::NoSetup => 0,
        probe::Mode::Quarter => attempted.div_ceil(4),
        probe::Mode::Half => attempted.div_ceil(2),
    };
    if submitted != expected {
        return Err(format!("draw suppression count differs: {submitted} != {expected}").into());
    }
    Ok(Sample {
        render_ms,
        completion_ms,
        attempted,
        submitted,
        stats,
        metrics,
    })
}

fn workload(
    label: &str,
    scene: &mut Scene,
    renderer: &mut Renderer,
    context: &crate::egl::Context,
    step: (usize, f64),
    output: &Path,
) -> Result<()> {
    use probe::Mode::*;
    let modes = [Normal, NoDraws, Quarter, Half, Discard, NoSetup];
    let (count, dt) = step;
    for _ in 0..5 {
        for mode in modes {
            render(scene, renderer, context, mode, false)?;
        }
    }
    let mut rows = Vec::with_capacity(count * 7);
    for index in 0..count {
        renderer.begin_frame();
        let started = Instant::now();
        scene.update(dt)?;
        let update_ms = started.elapsed().as_secs_f64() * 1000.0;
        let first = render(scene, renderer, context, Normal, false)?;
        let signature = (first.attempted, first.stats.triangles);
        let reference = if index == 0 || index + 1 == count {
            Some(measurement::capture(
                context,
                output,
                &format!("{label}-{index}-before"),
            )?)
        } else {
            None
        };
        rows.push((index, "normal-before", update_ms, first));
        // Rotate and reverse interventions to avoid systematically favoring a mode.
        for offset in 0..5 {
            let offset = if index % 2 == 0 {
                offset
            } else {
                4 - offset
            };
            let mode = modes[1 + (offset + index) % 5];
            let sample = render(scene, renderer, context, mode, false)?;
            if (sample.attempted, sample.stats.triangles) != signature {
                return Err("intervention changed scene preparation or logical draw counts".into());
            }
            rows.push((index, mode.name(), update_ms, sample));
        }
        let last = render(scene, renderer, context, Normal, false)?;
        if (last.attempted, last.stats.triangles) != signature {
            return Err("normal control changed logical draw counts".into());
        }
        if let Some(reference) = reference {
            let restored =
                measurement::capture(context, output, &format!("{label}-{index}-after"))?;
            if reference != restored {
                return Err("draw interventions changed the restored normal capture".into());
            }
        }
        rows.push((index, "normal-after", update_ms, last));
    }
    let mut csv = std::fs::File::create(output.join(format!("{label}-draw-sweep.csv")))?;
    writeln!(
        csv,
        "frame,mode,update_ms,render_cpu_ms,completion_ms,attempted,submitted,forward,shadow,triangles"
    )?;
    for (index, mode, update_ms, sample) in &rows {
        writeln!(
            csv,
            "{index},{mode},{update_ms},{},{},{},{},{},{},{}",
            sample.render_ms,
            sample.completion_ms,
            sample.attempted,
            sample.submitted,
            sample.stats.draw_calls,
            sample.stats.shadow_draw_calls,
            sample.stats.triangles
        )?;
    }
    for mode in [
        "normal-before",
        "no-draws",
        "quarter-draws",
        "half-draws",
        "rasterizer-discard",
        "no-draws-or-setup",
        "normal-after",
    ] {
        let mut times: Vec<_> = rows
            .iter()
            .filter(|row| row.1 == mode)
            .map(|row| row.3.render_ms)
            .collect();
        times.sort_unstable_by(f64::total_cmp);
        let median = (times[(count - 1) / 2] + times[count / 2]) / 2.0;
        println!("{label} {mode}: render CPU median {median:.3} ms");
    }
    // Per-call clocks run only in this separate attribution window.
    let mut csv = std::fs::File::create(output.join(format!("{label}-gl-calls.csv")))?;
    writeln!(csv, "frame,mode,function,calls,cpu_ns,render_cpu_ms")?;
    for index in 0..10 {
        for mode in [Normal, NoDraws, NoSetup] {
            let sample = render(scene, renderer, context, mode, true)?;
            for &(id, name) in probe::names() {
                let metric = sample.metrics[id];
                writeln!(
                    csv,
                    "{index},{},{name},{},{},{}",
                    mode.name(),
                    metric.calls,
                    metric.ns,
                    sample.render_ms
                )?;
            }
        }
    }
    render(scene, renderer, context, Normal, false)?;
    Ok(())
}

pub(super) fn run(
    scene: &mut Scene,
    renderer: &mut Renderer,
    context: &crate::egl::Context,
    count: usize,
    output: &Path,
) -> Result<()> {
    workload(
        "held-default-bounds",
        scene,
        renderer,
        context,
        (count, 0.0),
        output,
    )?;
    let unculled = measurement::capture(context, output, "sweep-unculled")?;
    scene.use_authored_bounds()?;
    for _ in 0..5 {
        measurement::frame(scene, renderer, context, 0.0, true)?;
    }
    let culled = measurement::capture(context, output, "sweep-culled")?;
    if unculled != culled {
        return Err("draw sweep culling changed held pixels".into());
    }
    workload(
        "held-culled",
        scene,
        renderer,
        context,
        (count, 0.0),
        output,
    )?;
    scene.control(AnimationPlaybackControl::Seek(0.0))?;
    scene.control(AnimationPlaybackControl::Play)?;
    for _ in 0..200 {
        measurement::frame(scene, renderer, context, 1.0 / 60.0, true)?;
    }
    workload(
        "moving-culled",
        scene,
        renderer,
        context,
        (count, 1.0 / 60.0),
        output,
    )?;
    if measurement::capture(context, output, "sweep-moving")? == culled {
        return Err("draw sweep animation did not change pixels".into());
    }
    Ok(())
}

/// Stable stack boundary so sampling tools can exclude loading and warm-up.
#[inline(never)]
fn profile_window(
    scene: &mut Scene,
    renderer: &mut Renderer,
    context: &crate::egl::Context,
    count: usize,
    output: &Path,
) -> Result<()> {
    let result = measurement::measure(
        "profile-moving-render",
        scene,
        renderer,
        context,
        (1.0 / 60.0, true),
        count,
        output,
    );
    // Keep this boundary on the stack instead of tail-calling the measurement.
    std::hint::black_box(&result);
    result
}

pub(super) fn profile(
    scene: &mut Scene,
    renderer: &mut Renderer,
    context: &crate::egl::Context,
    count: usize,
    output: &Path,
) -> Result<()> {
    scene.use_authored_bounds()?;
    scene.control(AnimationPlaybackControl::Seek(0.0))?;
    scene.control(AnimationPlaybackControl::Play)?;
    for _ in 0..200 {
        measurement::frame(scene, renderer, context, 1.0 / 60.0, true)?;
    }
    let before = measurement::capture(context, output, "profile-before")?;
    profile_window(scene, renderer, context, count, output)?;
    if measurement::capture(context, output, "profile-after")? == before {
        return Err("profile window animation did not change pixels".into());
    }
    Ok(())
}
