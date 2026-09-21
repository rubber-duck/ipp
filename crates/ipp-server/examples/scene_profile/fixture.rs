use super::{Renderer, Result};
use ipp_core::{
    Batch, Command, ComponentValue, EntityId, EntityRef, HostRuntime, WorldId,
    components::BoundingGeometry,
    services::asset_management::AssetLoadStatus,
    systems::animation::{AnimationControllerId, AnimationPlaybackControl},
};
use std::{collections::BTreeMap, path::Path, time::Instant};

pub(super) struct Scene {
    pub host: HostRuntime,
    pub world: WorldId,
    pub controllers: Vec<AnimationControllerId>,
    pub entities: BTreeMap<String, EntityId>,
    pub bounds: Vec<EntityId>,
    pub drivers: usize,
}

impl Scene {
    pub fn load(bundle: &Path, renderer: &Renderer) -> Result<Self> {
        let mut host = HostRuntime::new();
        renderer.install(&mut host)?;
        let prefix = "https://stress.ipp.invalid/";
        host.data_sources_mut().register(
            prefix,
            ipp_server::services::data_source::FileSystemDataSource::new(prefix, bundle, false)?,
        )?;
        let world = host.load_world(
            &std::fs::read(bundle.join("benchmark.ipp"))?,
            ipp_protocol::schema_hash(),
            Default::default(),
            Default::default(),
            Default::default(),
        )?;
        let view = host.world_mut(world).unwrap();
        let snapshots = view.entities();
        let entities: BTreeMap<_, _> = snapshots
            .iter()
            .filter_map(|entity| {
                entity
                    .metadata
                    .symbolic_id
                    .clone()
                    .map(|name| (name, entity.id))
            })
            .collect();
        let bounds = snapshots
            .iter()
            .filter(|entity| {
                let has = |kind| {
                    entity
                        .base
                        .iter()
                        .any(|component| component.type_id() == kind)
                };
                has(ComponentValue::MESH_INSTANCE)
                    && !has(ComponentValue::BOUNDING_GEOMETRY)
                    && !has(ComponentValue::PARTICLE_EMITTER)
                    && !has(ComponentValue::PARTICLE_PLAYBACK)
            })
            .map(|entity| entity.id)
            .collect();
        let descriptions = view.animation_controllers();
        let drivers = descriptions
            .iter()
            .map(|controller| controller.description.drivers.len())
            .sum();
        let controllers = descriptions
            .iter()
            .map(|controller| controller.id)
            .collect();
        drop(view);
        let mut scene = Self {
            host,
            world,
            entities,
            bounds,
            controllers,
            drivers,
        };
        let started = Instant::now();
        loop {
            scene.update(0.0)?;
            let mut pending = false;
            for asset in scene.host.asset_resources().iter() {
                match asset.status() {
                    AssetLoadStatus::Failed(error) => {
                        return Err(format!("asset {:?}: {error}", asset.source()).into());
                    }
                    AssetLoadStatus::Loaded => {}
                    _ => pending = true,
                }
            }
            if !pending {
                break;
            }
            if started.elapsed().as_secs() > 600 {
                return Err("native asset loading timed out".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        scene.control(AnimationPlaybackControl::Play)?;
        scene.control(AnimationPlaybackControl::Pause)?;
        scene.control(AnimationPlaybackControl::Seek(0.5))?;
        let camera = std::fs::read_to_string(bundle.join("camera.txt"))?;
        scene
            .host
            .world_mut(world)
            .unwrap()
            .enqueue_camera_activate(
                *scene
                    .entities
                    .get(camera.trim())
                    .ok_or("missing benchmark camera")?,
            )?;
        scene.update(0.0)?;
        println!(
            "Loaded {} entities, {} controllers, {} drivers, {} assets in {:.3} seconds",
            snapshots.len(),
            scene.controllers.len(),
            scene.drivers,
            scene.host.asset_resources().iter().count(),
            started.elapsed().as_secs_f64()
        );
        Ok(scene)
    }

    pub fn update(&mut self, dt: f64) -> Result<()> {
        self.host
            .world_mut(self.world)
            .unwrap()
            .prepare_update(dt)?;
        self.host.progress_assets();
        let report = self.host.world_mut(self.world).unwrap().step(dt)?;
        if report
            .outcomes
            .iter()
            .any(|outcome| outcome.result.is_err())
            || report
                .system_command_outcomes
                .iter()
                .any(|outcome| outcome.result.is_err())
        {
            return Err("native benchmark mutation failed".into());
        }
        Ok(())
    }

    pub fn control(&mut self, control: AnimationPlaybackControl) -> Result<()> {
        for start in (0..self.controllers.len()).step_by(32) {
            for &controller in &self.controllers[start..(start + 32).min(self.controllers.len())] {
                self.host
                    .world_mut(self.world)
                    .unwrap()
                    .enqueue_playback(controller, control)?;
            }
            self.update(0.0)?;
        }
        Ok(())
    }

    pub fn use_authored_bounds(&mut self) -> Result<()> {
        for (index, chunk) in self.bounds.chunks(4096).enumerate() {
            self.host.world_mut(self.world).unwrap().enqueue(Batch {
                id: index as u64 + 1,
                operations: chunk
                    .iter()
                    .map(|&id| Command::InsertComponentValue {
                        entity: EntityRef::Handle(id),
                        value: ComponentValue::BoundingGeometry(BoundingGeometry::default()),
                    })
                    .collect(),
            })?;
        }
        self.update(0.0)
    }

    pub fn validate_probes(&mut self, path: &Path) -> Result<()> {
        let probes = std::fs::read_to_string(path)?;
        let mut previous = None;
        let mut count = 0;
        for row in probes.lines() {
            let fields: Vec<_> = row.split('\t').collect();
            if fields.len() != 9 {
                return Err("invalid Blender probe row".into());
            }
            let expected = fields[1..]
                .iter()
                .map(|field| field.parse::<f64>())
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if previous != Some(expected[0]) {
                self.control(AnimationPlaybackControl::Pause)?;
                self.control(AnimationPlaybackControl::Seek(expected[0]))?;
                previous = Some(expected[0]);
            }
            let entity = *self.entities.get(fields[0]).ok_or("missing probe entity")?;
            let snapshot = self
                .host
                .world_mut(self.world)
                .unwrap()
                .inspect(entity)
                .ok_or("missing probe state")?;
            let transform = snapshot
                .effective
                .iter()
                .find_map(|value| {
                    if let ComponentValue::Transform(transform) = value {
                        Some(transform)
                    } else {
                        None
                    }
                })
                .ok_or("missing probe transform")?;
            for (actual, expected) in [transform.x, transform.y, transform.z]
                .into_iter()
                .zip(&expected[1..4])
            {
                if (f64::from(actual) - expected).abs() > 0.0001 {
                    return Err(format!(
                        "Blender position mismatch: {} at {}",
                        fields[0], fields[1]
                    )
                    .into());
                }
            }
            let rotation = [transform.qx, transform.qy, transform.qz, transform.qw];
            let error = [1.0, -1.0]
                .into_iter()
                .map(|sign| {
                    rotation
                        .iter()
                        .zip(&expected[4..])
                        .map(|(&a, &b)| (f64::from(a) - b * sign).abs())
                        .fold(0.0, f64::max)
                })
                .fold(f64::INFINITY, f64::min);
            if error > 0.00001 {
                return Err(
                    format!("Blender rotation mismatch: {} at {}", fields[0], fields[1]).into(),
                );
            }
            count += 1;
        }
        if count == 0 {
            return Err("empty Blender probes".into());
        }
        println!("Verified {count} independently sampled Blender position/rotation probes");
        self.validate_pose_probes(&path.with_file_name("pose-probes.tsv"))
    }
}
