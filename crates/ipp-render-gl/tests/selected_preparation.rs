//! Exact presentation demand through real Host resource ownership; GLES owns pixel evidence.

mod support;

use ipp_core::{
    ComponentValue, HostRuntime, OutputKind, OutputRef, WorldPublicationId, WorldViewport,
    components::{BoundingGeometry, Camera, Transform},
    services::asset_management::{AssetKey, AssetSource, AssetTypeId},
    systems::geometry::{GeometryDefinition, GeometryShape},
};
use ipp_render_gl::{RenderError, RenderService};
use std::{collections::BTreeSet, rc::Rc};
use support::{DeviceState, TestDevice, create};

fn viewport() -> WorldViewport {
    WorldViewport {
        width: 100,
        height: 100,
        device_pixel_ratio: 1.0,
    }
}

fn root(host: &mut HostRuntime, debug: bool) -> OutputRef {
    let world = host
        .create_world(Default::default(), &support::scene_systems(host))
        .unwrap();
    let entity = create(
        &mut host.world_mut(world).unwrap(),
        vec![
            ComponentValue::Camera(Camera::default()),
            ComponentValue::Transform(Transform {
                z: 5.0,
                ..Default::default()
            }),
        ],
    );
    if debug {
        let geometry = GeometryDefinition::from(GeometryShape::Box {
            min: [-0.25; 3],
            max: [0.25; 3],
        })
        .encode()
        .unwrap();
        create(
            &mut host.world_mut(world).unwrap(),
            vec![ComponentValue::BoundingGeometry(BoundingGeometry {
                geometry,
                is_rendered: true,
                ..Default::default()
            })],
        );
    }
    let output = host
        .bind_output(host.world_ref(world).unwrap(), entity, OutputKind::Camera)
        .unwrap();
    host.set_root_output(output, viewport()).unwrap();
    output
}

fn selected(host: &HostRuntime, output: OutputRef) -> Option<(OutputRef, WorldPublicationId)> {
    host.root_output(output.world().id())
        .map(|(output, _, publication)| (output, publication))
}

fn programs(host: &HostRuntime) -> BTreeSet<AssetKey> {
    host.asset_resources()
        .iter()
        .filter(|resource| resource.source().uri.starts_with("ipp-render://program/"))
        .map(|resource| resource.key())
        .collect()
}

fn prepare(renderer: &mut RenderService<TestDevice>, host: &mut HostRuntime, output: OutputRef) {
    let selected = selected(host, output);
    renderer.prepare(host, selected).unwrap();
}

fn fixture() -> (HostRuntime, RenderService<TestDevice>, Rc<DeviceState>) {
    let mut host = support::task_scheduler::host();
    let state = Rc::new(DeviceState::default());
    let renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    renderer.install(&mut host).unwrap();
    (host, renderer, state)
}

#[test]
fn preparation_is_selected_not_all_roots_and_unchanged_demand_stays_warm() {
    let (mut host, mut renderer, state) = fixture();
    let empty = root(&mut host, false);
    let drawn = root(&mut host, true);
    host.frame(0.0).unwrap();
    let tick = host.world_mut(drawn.world().id()).unwrap().tick();
    prepare(&mut renderer, &mut host, empty);
    support::progress_assets(&mut host);
    assert!(programs(&host).is_empty());
    assert_eq!(state.program_creates.get(), 0);
    assert_eq!(host.world_mut(drawn.world().id()).unwrap().tick(), tick);
    prepare(&mut renderer, &mut host, drawn);
    support::progress_assets(&mut host);
    let original = programs(&host);
    assert_eq!(original.len(), 1);
    assert_eq!(state.program_creates.get(), 1);
    for _ in 0..3 {
        prepare(&mut renderer, &mut host, drawn);
        support::progress_assets(&mut host);
        assert_eq!(programs(&host), original);
    }
    assert_eq!(state.program_creates.get(), 1);
    let publication = selected(&host, drawn).unwrap().1;
    assert_eq!(
        renderer
            .draw(&host, drawn, publication, viewport(), 0.0)
            .unwrap()
            .draw_calls,
        1
    );
    assert_eq!(state.live_meshes.get(), 1);
    prepare(&mut renderer, &mut host, empty);
    host.flush_resource_lifecycle();
    assert!(programs(&host).is_empty());
    assert_eq!(state.program_deletes.get(), 1);
    assert_eq!(state.live_meshes.get(), 0);
    renderer.prepare(&mut host, None).unwrap();
    assert!(host.root_output(drawn.world().id()).is_some());
}

#[test]
fn deselection_preserves_explicit_ownership_by_another_world() {
    let (mut host, mut renderer, _) = fixture();
    let drawn = root(&mut host, true);
    let independent = root(&mut host, false);
    host.frame(0.0).unwrap();
    prepare(&mut renderer, &mut host, drawn);
    support::progress_assets(&mut host);
    let key = *programs(&host).first().unwrap();
    let source = host.asset_resources().get(key).unwrap().source().clone();
    host.asset_resources_mut()
        .retain_prepared_source(independent.world().id(), &source)
        .unwrap();
    renderer.prepare(&mut host, None).unwrap();
    host.flush_resource_lifecycle();
    assert!(host.asset_resources().get(key).is_some());
    host.asset_resources_mut()
        .release_client_source(independent.world().id(), &source);
    host.flush_resource_lifecycle();
    assert!(host.asset_resources().get(key).is_none());
}

#[test]
fn stale_selection_and_destroyed_world_clear_own_demand_without_retiring_the_host() {
    let (mut host, mut renderer, state) = fixture();
    let output = root(&mut host, true);
    host.frame(0.0).unwrap();
    let previous = selected(&host, output);
    renderer.prepare(&mut host, previous).unwrap();
    support::progress_assets(&mut host);
    host.clear_root_output(output.world().id());
    assert!(host.output(previous.unwrap().1, output).is_some());
    assert_eq!(
        renderer.prepare(&mut host, previous),
        Err(RenderError::UnavailableOutput)
    );
    host.flush_resource_lifecycle();
    assert!(programs(&host).is_empty());
    host.set_root_output(output, viewport()).unwrap();
    host.frame(0.0).unwrap();
    assert_eq!(
        renderer.prepare(&mut host, previous),
        Err(RenderError::UnavailableOutput)
    );
    prepare(&mut renderer, &mut host, output);
    support::progress_assets(&mut host);
    assert_eq!(state.program_creates.get(), 2);
    assert!(host.destroy_world(output.world().id()));
    assert_eq!(
        renderer.prepare(&mut host, previous),
        Err(RenderError::UnavailableOutput)
    );
    assert!(programs(&host).is_empty());
    let healthy = root(&mut host, true);
    host.frame(0.0).unwrap();
    prepare(&mut renderer, &mut host, healthy);
    support::progress_assets(&mut host);
    assert_eq!(programs(&host).len(), 1);
}

#[test]
fn foreign_host_world_id_collision_requires_explicit_deselection_without_mutation() {
    let (mut first, mut renderer, state) = fixture();
    let output = root(&mut first, true);
    first.frame(0.0).unwrap();
    prepare(&mut renderer, &mut first, output);
    support::progress_assets(&mut first);
    let publication = selected(&first, output).unwrap().1;
    renderer
        .draw(&first, output, publication, viewport(), 0.0)
        .unwrap();
    assert_eq!(state.live_meshes.get(), 1);
    let mut second = support::task_scheduler::host();
    renderer.install(&mut second).unwrap();
    let foreign = root(&mut second, false);
    assert_eq!(output.world().id(), foreign.world().id());
    assert_ne!(output.world(), foreign.world());
    let source = AssetSource {
        kind: AssetTypeId(14),
        uri: "unrelated://private-program".into(),
        variant: 0,
    };
    let key = second
        .asset_resources_mut()
        .prepare_internal_source(
            foreign.world().id(),
            source.clone(),
            0u32.to_le_bytes().to_vec(),
        )
        .unwrap();
    second.frame(0.0).unwrap();
    let foreign_selection = selected(&second, foreign);
    assert_eq!(
        renderer.prepare(&mut second, None),
        Err(RenderError::HostCatalogMismatch)
    );
    assert_eq!(
        renderer.prepare(&mut second, foreign_selection),
        Err(RenderError::HostCatalogMismatch)
    );
    second.flush_resource_lifecycle();
    assert_eq!(second.asset_resources().find(&source), Some(key));
    assert_eq!(programs(&first).len(), 1);
    assert_eq!(state.live_meshes.get(), 1);
    support::progress_assets(&mut second);
    let foreign_status = second.asset_resources().get(key).unwrap().status().clone();
    let deleted_programs = state.program_deletes.get();
    assert_eq!(
        renderer.unload_host(&mut second),
        Err(RenderError::HostCatalogMismatch)
    );
    assert_eq!(
        renderer.replace_device(&mut second, TestDevice(state.clone())),
        Err(RenderError::HostCatalogMismatch)
    );
    assert_eq!(state.live_meshes.get(), 1);
    assert_eq!(state.program_deletes.get(), deleted_programs);
    assert_eq!(
        second.asset_resources().get(key).unwrap().status(),
        &foreign_status
    );
    assert_eq!(
        renderer.draw(
            &second,
            foreign,
            foreign_selection.unwrap().1,
            viewport(),
            0.0
        ),
        Err(RenderError::HostCatalogMismatch)
    );
    assert_eq!(
        renderer.prepare(&mut second, selected(&first, output)),
        Err(RenderError::HostCatalogMismatch)
    );
    renderer.prepare(&mut first, None).unwrap();
    first.flush_resource_lifecycle();
    assert!(programs(&first).is_empty());
    assert_eq!(second.asset_resources().find(&source), Some(key));
    renderer.prepare(&mut second, foreign_selection).unwrap();
    renderer.prepare(&mut second, None).unwrap();
    second.flush_resource_lifecycle();
    assert_eq!(second.asset_resources().find(&source), Some(key));
}

#[test]
fn destroyed_selected_world_then_none_releases_metadata_for_repeated_catalog_reuse() {
    let state = Rc::new(DeviceState::default());
    let mut renderer = RenderService::new(TestDevice(state.clone())).unwrap();
    for _ in 0..32 {
        let mut host = support::task_scheduler::host();
        renderer.install(&mut host).unwrap();
        let output = root(&mut host, true);
        host.frame(0.0).unwrap();
        prepare(&mut renderer, &mut host, output);
        support::progress_assets(&mut host);
        assert_eq!(programs(&host).len(), 1);
        assert!(host.destroy_world(output.world().id()));
        assert!(programs(&host).is_empty());
        renderer.prepare(&mut host, None).unwrap();
        assert_eq!(host.world_ids().count(), 0);
    }
    assert_eq!(state.program_creates.get(), 32);
    assert_eq!(state.program_deletes.get(), 32);
}

#[test]
fn nested_camera_demand_and_targets_follow_only_the_selected_root() {
    let (mut host, mut renderer, state) = fixture();
    state.cache_limit.set(256);
    let parent = root(&mut host, false);
    let child = root(&mut host, true);
    host.clear_root_output(child.world().id());
    create(
        &mut host.world_mut(parent.world().id()).unwrap(),
        vec![
            ComponentValue::Transform(Default::default()),
            ComponentValue::Surface(Default::default()),
            ComponentValue::WorldAttachment(ipp_core::WorldAttachment::surface(child)),
        ],
    );
    host.frame(0.0).unwrap();
    let selected = selected(&host, parent).unwrap();
    renderer.prepare(&mut host, Some(selected)).unwrap();
    support::progress_assets(&mut host);
    assert_eq!(programs(&host).len(), 1);
    renderer
        .draw(&host, selected.0, selected.1, viewport(), 0.0)
        .unwrap();
    assert_eq!(state.cache_targets_live.get(), 1);
    renderer.prepare(&mut host, None).unwrap();
    host.flush_resource_lifecycle();
    assert!(programs(&host).is_empty());
    assert_eq!(state.cache_targets_live.get(), 0);
    assert!(host.world_ref(child.world().id()).is_some());
}
