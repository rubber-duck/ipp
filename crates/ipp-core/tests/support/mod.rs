//! Direct-core fixture driver: explicit Host service progress, then one world update.
//! The maintained native/browser harness establishes production transport/render behavior.

#![allow(dead_code)]

use ipp_core::{ErrorReason, WorldContext, WorldUpdateReport};

pub trait WorldTestDriver {
    fn update_for_test(&mut self, dt: f64) -> Result<WorldUpdateReport, ErrorReason>;

    fn resource_requests_for_test(&mut self) -> Vec<ipp_core::AssetAcquisitionRequest>;

    fn await_upload_for_test(&mut self) -> WorldUpdateReport;
}

impl WorldTestDriver for WorldContext<'_> {
    fn update_for_test(&mut self, dt: f64) -> Result<WorldUpdateReport, ErrorReason> {
        self.prepare_update(dt)?;
        self.poll_assets();
        self.step(dt)
    }

    fn resource_requests_for_test(&mut self) -> Vec<ipp_core::AssetAcquisitionRequest> {
        self.poll_assets();
        self.take_resource_requests()
    }

    fn await_upload_for_test(&mut self) -> WorldUpdateReport {
        for _ in 0..512 {
            let report = self.update_for_test(0.0).unwrap();
            if !report.assets.is_empty() {
                return report;
            }
        }
        panic!("asset upload did not finish");
    }
}

/// Own the complete Host boundary when a fixture releases or retries shared resources.
pub trait HostWorldTestDriver {
    fn update_world_for_test(
        &mut self,
        world: ipp_core::WorldId,
        dt: f64,
    ) -> Result<WorldUpdateReport, ErrorReason>;

    fn await_world_upload_for_test(&mut self, world: ipp_core::WorldId) -> WorldUpdateReport;
}

impl HostWorldTestDriver for ipp_core::HostRuntime {
    fn update_world_for_test(
        &mut self,
        world: ipp_core::WorldId,
        dt: f64,
    ) -> Result<WorldUpdateReport, ErrorReason> {
        self.world_mut(world)
            .expect("fixture world")
            .prepare_update(dt)?;
        self.progress_assets();
        let report = self.world_mut(world).expect("fixture world").step(dt)?;
        self.flush_resource_lifecycle();
        Ok(report)
    }

    fn await_world_upload_for_test(&mut self, world: ipp_core::WorldId) -> WorldUpdateReport {
        for _ in 0..512 {
            let report = self.update_world_for_test(world, 0.0).unwrap();
            if !report.assets.is_empty() {
                return report;
            }
        }
        panic!("asset upload did not finish through Host lifecycle");
    }
}

/// Compact CPU metadata retained for these unskinned fixture meshes, separate
/// from vertex/index streams. Mesh-pose builds also retain topology for matching.
pub fn unskinned_mesh_metadata_bytes(index_count: usize) -> usize {
    std::mem::size_of::<ipp_core::services::asset_management::mesh_metadata::MeshMetadata>()
        + if cfg!(feature = "mesh-poses") {
            index_count * 2
        } else {
            0
        }
}
