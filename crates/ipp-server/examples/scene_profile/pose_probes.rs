//! Independent Blender deformation samples checked outside timing windows.
use super::{Result, fixture::Scene};
use ipp_core::{ComponentValue, systems::animation::AnimationPlaybackControl};
use std::path::Path;

impl Scene {
    pub(super) fn validate_pose_probes(&mut self, path: &Path) -> Result<()> {
        let probes = std::fs::read_to_string(path)?;
        let mut previous = None;
        let mut count = 0;
        self.control(AnimationPlaybackControl::Pause)?;

        for row in probes.lines() {
            let fields: Vec<_> = row.split('\t').collect();
            if fields.len() != 6 {
                return Err("invalid Blender vertex probe row".into());
            }

            let values = fields[1..]
                .iter()
                .map(|value| value.parse::<f64>())
                .collect::<std::result::Result<Vec<_>, _>>()?;
            if previous != Some(values[0]) {
                self.control(AnimationPlaybackControl::Seek(values[0]))?;
                previous = Some(values[0]);
            }

            let entity = *self.entities.get(fields[0]).ok_or("missing pose entity")?;
            let world = self.host.world_mut(self.world).unwrap();
            let snapshot = world.inspect(entity).ok_or("missing pose state")?;
            let weight = snapshot
                .effective
                .iter()
                .find_map(|value| match value {
                    ComponentValue::MeshPose(pose) => Some(pose.weight),
                    _ => None,
                })
                .ok_or("missing MeshPose component")?;
            if (f64::from(weight) - values[1]).abs() > 0.0002 {
                return Err(format!(
                    "Blender mesh-pose weight mismatch: {} at {}",
                    fields[0], fields[1]
                )
                .into());
            }

            let bounds = world
                .mesh_bounds(entity)?
                .ok_or("missing deformed mesh bounds")?;
            for axis in 0..3 {
                let vertex = values[axis + 2];
                if !vertex.is_finite()
                    || vertex < bounds[0][axis] - 0.001
                    || vertex > bounds[1][axis] + 0.001
                {
                    return Err(format!("Blender deformed vertex outside bounds: {} at {}, axis {axis}: {vertex}, bounds {bounds:?}", fields[0], fields[1]).into());
                }
            }
            count += 1;
        }

        if count == 0 {
            return Err("empty Blender vertex probes".into());
        }

        println!(
            "Verified {count} Blender deformed vertices inside runtime mesh bounds (rigid pose and pose-before-skin)"
        );
        Ok(())
    }
}
