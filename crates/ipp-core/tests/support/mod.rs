//! Direct-core fixture driver: explicit Host service progress, then one world update.
//! The maintained native/browser harness establishes production transport/render behavior.

#![allow(dead_code)]

pub mod selection;
pub mod world_failures;

use ipp_core::{ErrorReason, WorldContext, WorldUpdateReport};

pub mod gui_panel;

/// Select a World's canvas with its stored extent and density.
pub trait CanvasTestHost {
    /// Queue the canvas state for the next mutation boundary and return the
    /// World's canvas output.
    fn canvas_output(
        &mut self,
        world: ipp_core::WorldRef,
        extent: [f32; 2],
        density: f32,
    ) -> ipp_core::OutputRef;
}

impl CanvasTestHost for ipp_core::HostRuntime {
    fn canvas_output(
        &mut self,
        world: ipp_core::WorldRef,
        extent: [f32; 2],
        density: f32,
    ) -> ipp_core::OutputRef {
        self.world_mut(world.id())
            .unwrap()
            .enqueue_canvas_state_update(ipp_core::CanvasStateUpdate {
                extent: Some(extent),
                units_per_metre: Some(density),
            })
            .unwrap();
        ipp_core::OutputRef::canvas(world)
    }
}

/// The first top-level entity of a World: the root a canvas fixture creates.
pub fn top_level_root(
    host: &mut ipp_core::HostRuntime,
    world: ipp_core::WorldId,
) -> ipp_core::EntityId {
    host.world_mut(world)
        .unwrap()
        .entity_children(None)
        .next()
        .expect("a top-level root")
}

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
/// from vertex/index streams, including the topology retained for pose matching.
pub fn unskinned_mesh_metadata_bytes(index_count: usize) -> usize {
    std::mem::size_of::<ipp_core::services::asset_management::mesh_metadata::MeshMetadata>()
        + index_count * 2
}

/// Minimal immutable font: fallback and A, with independently known advance metrics.
pub fn canvas_font_bytes() -> Vec<u8> {
    let mut bytes = b"IPPF".to_vec();
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(1000_u32.to_le_bytes());
    for value in [800.0_f32, -200.0, 200.0] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend(2_u32.to_le_bytes());
    bytes.extend(1_u32.to_le_bytes());
    bytes.extend(0_u32.to_le_bytes());
    for advance in [500.0_f32, 600.0] {
        bytes.extend(advance.to_le_bytes());
        for _ in 0..5 {
            bytes.extend(0.0_f32.to_le_bytes());
        }
        bytes.extend(0_u32.to_le_bytes());
    }
    bytes.extend(u32::from('A').to_le_bytes());
    bytes.extend(1_u32.to_le_bytes());
    bytes
}
