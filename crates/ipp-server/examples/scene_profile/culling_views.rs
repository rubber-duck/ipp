//! Camera-view comparisons on the same animated scene and completed geometry.
use super::{HEIGHT, Renderer, Result, WIDTH, fixture::Scene, measurement};
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityMetadata, EntityRef,
    components::{Camera, Transform},
    systems::{
        animation::AnimationPlaybackControl,
        geometry::{
            GeometryQueryResults, GeometryQueryScratch, GeometrySpatialBackend, frustum_planes,
        },
    },
};
use std::{collections::BTreeSet, io::Write, path::Path, time::Instant};

fn insert(entity: EntityRef, value: ComponentValue) -> Command {
    Command::InsertComponentValue {
        entity,
        value,
    }
}

fn camera(scene: &mut Scene, projection: Camera) -> Result<EntityId> {
    let mut world = scene.host.world_mut(scene.world).unwrap();
    world.enqueue(Batch {
        id: world.tick() + 1,
        operations: vec![
            Command::Create {
                alias: 0,
                metadata: EntityMetadata {
                    symbolic_id: Some("benchmark-interior-camera".into()),
                    classes: vec![],
                },
            },
            insert(
                EntityRef::Alias(0),
                ComponentValue::Transform(Transform::default()),
            ),
            insert(EntityRef::Alias(0), ComponentValue::Camera(projection)),
        ],
    })?;
    drop(world);
    scene.update(0.0)?;
    scene
        .host
        .world_mut(scene.world)
        .unwrap()
        .entities()
        .iter()
        .find(|e| e.metadata.symbolic_id.as_deref() == Some("benchmark-interior-camera"))
        .map(|e| e.id)
        .ok_or_else(|| "interior camera creation failed".into())
}

fn select(scene: &mut Scene, entity: EntityId, yaw: f32, projection: Camera) -> Result<()> {
    // Fixed camera station at the grid centre. Keep all original animation drivers,
    // including the overview camera's, running identically in every workload.
    let (sp, cp) = (-5.0_f32.to_radians()).sin_cos();
    let (sy, cy) = (yaw * 0.5).sin_cos();
    let mut world = scene.host.world_mut(scene.world).unwrap();
    world.enqueue(Batch {
        id: world.tick() + 1,
        operations: vec![
            insert(
                EntityRef::Handle(entity),
                ComponentValue::Transform(Transform {
                    y: 2.2,
                    qx: cy * sp,
                    qy: sy * cp,
                    qz: -sy * sp,
                    qw: cy * cp,
                    ..Transform::default()
                }),
            ),
            insert(
                EntityRef::Handle(entity),
                ComponentValue::Camera(projection),
            ),
        ],
    })?;
    world.enqueue_camera_activate(entity)?;
    drop(world);
    scene.update(0.0)
}

fn query_comparison(
    scene: &mut Scene,
    renderer: &mut Renderer,
    context: &crate::egl::Context,
    label: &str,
    output: &Path,
    rows: &mut std::fs::File,
) -> Result<()> {
    let mut reference = None;
    let mut expected = None;
    for (name, backend) in [
        ("bvh", GeometrySpatialBackend::Bvh),
        ("flat", GeometrySpatialBackend::Flat),
    ] {
        scene
            .host
            .world_mut(scene.world)
            .unwrap()
            .set_geometry_spatial_backend(backend);
        for _ in 0..5 {
            measurement::frame(scene, renderer, context, 0.0, true)?;
        }
        let pixels = measurement::capture(context, output, &format!("{label}-{name}"))?;
        if reference.as_ref().is_some_and(|r| *r != pixels) {
            return Err(format!("{label}: flat scan and BVH pixels differ").into());
        }
        reference = Some(pixels);
        let world = scene.host.world_mut(scene.world).unwrap();
        let frustum = frustum_planes(world.prepare_camera(WIDTH, HEIGHT)?.view_projection);
        let entities: BTreeSet<_> = world
            .render_items()
            .iter()
            .map(|item| item.entity)
            .collect();
        let index = world.geometry_spatial_index();
        let mut results = GeometryQueryResults::default();
        let mut scratch = GeometryQueryScratch::default();
        for _ in 0..8 {
            index.query_frustums(&[frustum], &mut results, &mut scratch);
        }
        let mut timings = Vec::with_capacity(256);
        for _ in 0..256 {
            let started = Instant::now();
            index.query_frustums(&[frustum], &mut results, &mut scratch);
            std::hint::black_box(&results);
            timings.push(started.elapsed().as_secs_f64() * 1e6);
        }
        timings.sort_unstable_by(f64::total_cmp);
        let visible: Vec<_> = entities
            .iter()
            .copied()
            .filter(|&e| results.matches(e, 0))
            .collect();
        if expected.as_ref().is_some_and(|v| *v != visible) {
            return Err(format!("{label}: flat scan and BVH identities differ").into());
        }
        for &entity in &entities {
            if !results.matches(entity, 0) && world.geometry_visible(entity, &frustum) {
                return Err(format!(
                    "{label}: index rejected a geometry-visible entity {entity:?}"
                )
                .into());
            }
        }
        let unknown = entities
            .iter()
            .filter(|&&e| index.get(e).is_none_or(|row| row.culling.is_none()))
            .count();
        let median = (timings[127] + timings[128]) * 0.5;
        writeln!(
            rows,
            "{label},{name},{},{},{unknown},{median}",
            entities.len(),
            visible.len()
        )?;
        println!(
            "{label} {name}: {} of {} renderable entities match camera bounds, {unknown} unknown; query {median:.3} us",
            visible.len(),
            entities.len()
        );
        expected = Some(visible);
    }
    scene
        .host
        .world_mut(scene.world)
        .unwrap()
        .set_geometry_spatial_backend(GeometrySpatialBackend::Bvh);
    Ok(())
}

pub(super) fn run(
    scene: &mut Scene,
    renderer: &mut Renderer,
    context: &crate::egl::Context,
    count: usize,
    output: &Path,
    bundle: &Path,
) -> Result<()> {
    let world = scene.host.world_mut(scene.world).unwrap();
    let projection = *world
        .active_camera_component()
        .ok_or("missing camera projection")?;
    drop(world);
    let mut queries = std::fs::File::create(output.join("camera-queries.csv"))?;
    writeln!(
        queries,
        "view,backend,renderable_entities,camera_candidates,unknown_bounds,query_median_us"
    )?;
    let mut cameras = std::fs::File::create(output.join("cameras.txt"))?;
    writeln!(
        cameras,
        "viewport={WIDTH}x{HEIGHT}; fov_y={}; near={}; original_far={}; interior_position=[0,2.2,0]; pitch_degrees=-10",
        projection.fov_y, projection.near, projection.far
    )?;
    for (label, yaw, far) in [
        ("overview", None, projection.far),
        ("interior", Some(0.0), projection.far),
        (
            "interior-turned",
            Some(std::f32::consts::FRAC_PI_2),
            projection.far,
        ),
        ("interior-30m", Some(0.0), 30.0),
    ] {
        // Reload every station: controller seeks do not rewind live particle time.
        // This also resets renderer selection history and resource identities.
        renderer.forget_world(scene.world);
        if !scene.host.destroy_world(scene.world) {
            return Err("camera comparison World reset failed".into());
        }
        renderer.unload_host(&mut scene.host);
        *renderer = Renderer::new(context.device()?)?;
        *scene = Scene::load(bundle, renderer)?;
        let overview = scene
            .host
            .world_mut(scene.world)
            .unwrap()
            .active_camera()
            .ok_or("missing reloaded overview camera")?;
        let interior = camera(scene, projection)?;
        scene.control(AnimationPlaybackControl::Pause)?;
        scene.control(AnimationPlaybackControl::Seek(0.0))?;
        if let Some(yaw) = yaw {
            select(
                scene,
                interior,
                yaw,
                Camera {
                    far,
                    ..projection
                },
            )?;
        } else {
            scene
                .host
                .world_mut(scene.world)
                .unwrap()
                .enqueue_camera_activate(overview)?;
            scene.update(0.0)?;
        }
        writeln!(cameras, "{label}: yaw={yaw:?}; far={far}")?;
        scene.control(AnimationPlaybackControl::Play)?;
        // Match the existing moving benchmark: settle for 200 advancing frames.
        for _ in 0..200 {
            measurement::frame(scene, renderer, context, 1.0 / 60.0, true)?;
        }
        measurement::measure(
            label,
            scene,
            renderer,
            context,
            (1.0 / 60.0, true),
            count,
            output,
        )?;
        scene.control(AnimationPlaybackControl::Pause)?;
        query_comparison(scene, renderer, context, label, output, &mut queries)?;
        scene.control(AnimationPlaybackControl::Play)?;
        measurement::measure(
            &format!("{label}-update"),
            scene,
            renderer,
            context,
            (1.0 / 60.0, false),
            count,
            output,
        )?;
        scene.control(AnimationPlaybackControl::Pause)?;
        scene
            .host
            .world_mut(scene.world)
            .unwrap()
            .enqueue_camera_activate(overview)?;
        scene.update(0.0)?;
    }
    Ok(())
}
