//! Real Host graph captures, private reconstruction and exact created-World receipts.

mod support;

use support::selection::{ATTACHMENTS, CAMERA, CONSTRAINTS, select};
use support::selection::{CANVAS, SURFACE};

use ipp_core::components::{Camera, Scalar};
use ipp_core::services::world_serialization::*;
use ipp_core::systems::*;
use ipp_core::*;
use std::collections::BTreeMap;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Default)]
struct DurableProbe {
    reject_second: AtomicBool,
    loads: AtomicUsize,
    teardowns: AtomicUsize,
}

struct DurableFactory(Arc<DurableProbe>);

struct DurableSystem(Arc<DurableProbe>);

impl SystemFactory for DurableFactory {
    fn id(&self) -> SystemId {
        SystemId("fixture.graph-durable")
    }

    fn create(&self, _: &mut SystemInitContext<'_>) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(DurableSystem(self.0.clone())))
    }
}

impl System for DurableSystem {
    fn update(&mut self, _: &mut SystemUpdateContext<'_, '_>) {}

    fn save_persistent_state(
        &self,
        context: &mut SystemSaveContext<'_>,
    ) -> Result<Option<SystemPersistentState>, String> {
        Ok(Some((context.ids.len() as u64).to_le_bytes().to_vec()))
    }

    fn load_persistent_state(
        &mut self,
        context: &mut SystemLoadContext<'_, '_>,
        state: Option<&SystemPersistentState>,
    ) -> Result<(), String> {
        let expected = (context.ids.len() as u64).to_le_bytes();
        if state.map(Vec::as_slice) != Some(expected.as_slice()) {
            return Err("Incorrect durable payload or missing reconstructed identities".into());
        }
        let previous = self.0.loads.fetch_add(1, Ordering::SeqCst);
        if self.0.reject_second.load(Ordering::SeqCst) && previous == 1 {
            return Err("Rejected after graph components were reconstructed".into());
        }
        Ok(())
    }

    fn teardown(&mut self, _: &mut SystemTeardownContext<'_>) {
        self.0.teardowns.fetch_add(1, Ordering::SeqCst);
    }
}

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();
    let outcome = host
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0);
    assert!(outcome.result.is_ok(), "{:?}", outcome.result);
    outcome
}

fn entity(host: &mut HostRuntime, world: WorldId, values: Vec<ComponentValue>) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 0,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(0), value)),
    );
    apply(host, world, operations).result.unwrap()[0].1
}

fn named(host: &mut HostRuntime, name: &str, systems: Vec<SystemId>) -> WorldId {
    host.create_world_with_options(
        Default::default(),
        WorldCreateOptions {
            symbolic_id: name.into(),
            ..WorldCreateOptions::new(systems)
        },
    )
    .unwrap()
}

fn fixture() -> (HostRuntime, WorldId, [WorldId; 2]) {
    let mut host = crate::support::task_scheduler::host();
    let root = named(&mut host, "root", select(&[ATTACHMENTS]));
    let first = named(&mut host, "left", select(&[CAMERA, CONSTRAINTS]));
    entity(
        &mut host,
        first,
        vec![
            ComponentValue::Camera(Camera::default()),
            ComponentValue::Scalar(Scalar {
                value: 3.0,
            }),
        ],
    );
    let bytes = host.save_world(first, 71, Default::default()).unwrap();
    let second = host
        .load_world(
            &bytes,
            71,
            WorldLoadOptions {
                symbolic_id: Some("right".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap()
        .root
        .id();
    for child in [first, second] {
        let attachment = WorldAttachment::spatial(host.world_ref(child).unwrap());
        entity(
            &mut host,
            root,
            vec![ComponentValue::WorldAttachment(attachment)],
        );
    }
    (host, root, [first, second])
}

fn copy_names(descriptor: &WorldGraphDescriptor) -> WorldLoadOptions {
    WorldLoadOptions {
        world_names: descriptor
            .nodes
            .iter()
            .map(|node| (node.id, format!("copy-{}", node.metadata.symbolic_id)))
            .collect(),
        ..Default::default()
    }
}

#[test]
fn graph_local_nodes_preserve_sibling_copies_and_acknowledge_every_independent_world() {
    let (mut host, root, children) = fixture();
    host.frame(0.0).unwrap();
    let original_tokens: Vec<_> = host
        .publication(host.latest_publication(root).unwrap())
        .unwrap()
        .attachments
        .iter()
        .map(|edge| edge.token.clone())
        .collect();
    let persistent = host
        .world_mut(children[0])
        .unwrap()
        .metadata()
        .persistent_id;
    assert_eq!(
        host.world_mut(children[1])
            .unwrap()
            .metadata()
            .persistent_id,
        persistent
    );
    let before = host.list_worlds();
    let bytes = host.save_world(root, 71, Default::default()).unwrap();
    let descriptor = inspect_world_graph(&bytes, 71, Default::default()).unwrap();
    assert_eq!(descriptor.nodes.len(), 3);
    assert_eq!(
        descriptor
            .nodes
            .iter()
            .filter(|node| node.metadata.persistent_id == persistent)
            .count(),
        2
    );
    assert_eq!(host.list_worlds(), before);
    assert!(
        host.load_world(
            &bytes,
            71,
            Default::default(),
            Default::default(),
            Default::default()
        )
        .is_err()
    );
    assert_eq!(host.list_worlds(), before);
    let loaded = host
        .load_world(
            &bytes,
            71,
            copy_names(&descriptor),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(loaded.created.len(), 3);
    assert_eq!(loaded.created[&descriptor.root], loaded.root);
    assert!(
        loaded
            .created
            .values()
            .all(|world| !before.iter().any(|old| old.id == world.id()))
    );
    let loaded_entities = host.world_mut(loaded.root.id()).unwrap().entities();
    let attached: Vec<_> = loaded_entities
        .iter()
        .flat_map(|entity| &entity.components)
        .filter_map(|value| {
            if let ComponentValue::WorldAttachment(value) = value {
                value.child()
            } else {
                None
            }
        })
        .collect();
    assert_eq!(attached.len(), 2);
    assert_ne!(attached[0], attached[1]);
    assert!(
        attached
            .iter()
            .all(|child| loaded.created.values().any(|created| created == child))
    );
    host.frame(0.0).unwrap();
    let publication = host
        .publication(host.latest_publication(loaded.root.id()).unwrap())
        .unwrap();
    assert_eq!(publication.attachments.len(), 2);
    assert!(
        publication
            .attachments
            .iter()
            .all(|edge| host.attached_publication(edge).is_some())
    );
    for edge in &publication.attachments {
        assert!(original_tokens.iter().all(|token| token != &edge.token));
        assert_eq!(
            host.attachment_retirement(&edge.token),
            Ok(WorldAttachmentRetirement::Pending)
        );
    }
    let root_ref = loaded.root;
    drop(loaded);
    assert_eq!(host.world_ids().len(), 6);
    assert!(host.destroy_world(root_ref.id()));
    for child in attached {
        assert_eq!(host.world_ref(child.id()), Some(child));
    }
    for token in original_tokens {
        assert_eq!(
            host.attachment_retirement(&token),
            Ok(WorldAttachmentRetirement::Pending)
        );
    }
}

#[test]
fn graph_byte_budget_bounds_the_combined_cut_and_decoded_nodes() {
    let mut host = crate::support::task_scheduler::host();
    let root = named(&mut host, "budget-root", select(&[ATTACHMENTS]));
    let limits = WorldPersistenceLimits {
        max_bytes: 50_000,
    };
    for name in ["budget-left", "budget-right"] {
        let child = host
            .create_world_with_options(
                Default::default(),
                WorldCreateOptions {
                    symbolic_id: name.into(),
                    ..WorldCreateOptions::new([])
                },
            )
            .unwrap();
        apply(
            &mut host,
            child,
            (0..256)
                .map(|alias| Command::Create {
                    alias,
                    metadata: Default::default(),
                    adopt: false,
                })
                .collect(),
        );
        assert!(host.save_world(child, 71, limits).is_ok());
        let child_ref = host.world_ref(child).unwrap();
        entity(
            &mut host,
            root,
            vec![ComponentValue::WorldAttachment(WorldAttachment::spatial(
                child_ref,
            ))],
        );
    }
    assert!(host.capture_world_graph(root, limits).is_err());
    assert!(host.save_world(root, 71, limits).is_err());
    let bytes = host.save_world(root, 71, Default::default()).unwrap();
    assert!(bytes.len() < limits.max_bytes);
    assert!(inspect_world_graph(&bytes, 71, limits).is_err());
    assert!(inspect_world_graph(&bytes, 71, Default::default()).is_ok());
}

#[test]
fn applied_cut_is_detached_from_later_batches() {
    let (mut host, root, children) = fixture();
    let initial = host.capture_world_graph(root, Default::default()).unwrap();
    apply(
        &mut host,
        children[0],
        vec![Command::Create {
            alias: 0,
            metadata: Default::default(),
            adopt: false,
        }],
    );
    let current = host.capture_world_graph(root, Default::default()).unwrap();
    assert_eq!(
        current.nodes[1].world.entities.len(),
        initial.nodes[1].world.entities.len() + 1
    );
    let bytes = initial.encode(71, Default::default()).unwrap();
    assert_eq!(
        WorldGraphSnapshot::decode(&bytes, 71, Default::default()).unwrap(),
        initial
    );
}

#[test]
fn descriptor_bounds_names_and_failed_graph_validation_publish_nothing() {
    let (mut host, root, _) = fixture();
    let graph = host.capture_world_graph(root, Default::default()).unwrap();
    let bytes = graph.encode(71, Default::default()).unwrap();
    let before = host.list_worlds();
    assert!(inspect_world_graph(&bytes, 72, Default::default()).is_err());
    assert!(
        inspect_world_graph(
            &bytes,
            71,
            WorldPersistenceLimits {
                max_bytes: 31
            }
        )
        .is_err()
    );
    for length in 0..bytes.len() {
        assert!(inspect_world_graph(&bytes[..length], 71, Default::default()).is_err());
    }
    let descriptor = inspect_world_graph(&bytes, 71, Default::default()).unwrap();
    let mut options = copy_names(&descriptor);
    options.symbolic_id = Some("different-root".into());
    assert!(
        host.load_world(&bytes, 71, options, Default::default(), Default::default())
            .is_err()
    );
    let mut options = copy_names(&descriptor);
    options
        .world_names
        .insert(WorldGraphNodeId(u32::MAX), "unknown".into());
    assert!(
        host.load_world(&bytes, 71, options, Default::default(), Default::default())
            .is_err()
    );
    let mut invalid = graph.clone();
    invalid.nodes[2]
        .world
        .selected_systems
        .push("missing-system".into());
    let invalid = invalid.encode(71, Default::default()).unwrap();
    assert!(
        host.load_world(
            &invalid,
            71,
            copy_names(&descriptor),
            Default::default(),
            Default::default()
        )
        .is_err()
    );
    assert_eq!(host.list_worlds(), before);
    let mut cyclic = graph;
    let root_id = cyclic.root;
    if let WorldSerializedReferenceValue::World(target) = &mut cyclic.nodes[0].references[0].value {
        *target = root_id;
    }
    assert!(cyclic.encode(71, Default::default()).is_err());
    for version in [2u32, 3, 4, 5] {
        let mut obsolete = bytes.clone();
        obsolete[4..8].copy_from_slice(&version.to_le_bytes());
        assert!(inspect_world_graph(&obsolete, 71, Default::default()).is_err());
    }
}

#[test]
fn selected_none_world_survives_graph_load_without_extra_factories() {
    let mut host = crate::support::task_scheduler::host();
    let root = host
        .create_world_with_options(
            Default::default(),
            WorldCreateOptions {
                symbolic_id: "none".into(),
                ..WorldCreateOptions::new([])
            },
        )
        .unwrap();
    entity(&mut host, root, Vec::new());
    let bytes = host.save_world(root, 71, Default::default()).unwrap();
    let descriptor = inspect_world_graph(&bytes, 71, Default::default()).unwrap();
    let loaded = host
        .load_world(
            &bytes,
            71,
            copy_names(&descriptor),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(
        host.world_manifest(loaded.root.id())
            .unwrap()
            .systems()
            .is_empty()
    );
    assert_eq!(
        loaded.created,
        BTreeMap::from([(descriptor.root, loaded.root)])
    );
}

#[test]
fn camera_output_references_remap_exact_producers_without_restoring_root_presentation() {
    let mut host = crate::support::task_scheduler::host();
    let root = named(&mut host, "scene", select(&[ATTACHMENTS, CAMERA, SURFACE]));
    let child = named(&mut host, "camera-child", select(&[CAMERA, CONSTRAINTS]));
    let root_camera = entity(
        &mut host,
        root,
        vec![ComponentValue::Camera(Camera::default())],
    );
    let camera = entity(
        &mut host,
        child,
        vec![ComponentValue::Camera(Camera::default())],
    );
    entity(
        &mut host,
        child,
        vec![ComponentValue::Scalar(Scalar {
            value: 8.0,
        })],
    );
    let root_output = host
        .bind_output(
            host.world_ref(root).unwrap(),
            root_camera,
            OutputKind::Camera,
        )
        .unwrap();
    let output = host
        .bind_output(host.world_ref(child).unwrap(), camera, OutputKind::Camera)
        .unwrap();
    entity(
        &mut host,
        root,
        vec![
            ComponentValue::create(ComponentValue::FLAT_SURFACE).unwrap(),
            ComponentValue::WorldAttachment(WorldAttachment::surface(output)),
        ],
    );
    host.set_root_output(
        root_output,
        WorldViewport {
            width: 640,
            height: 480,
            device_pixel_ratio: 2.0,
        },
    )
    .unwrap();
    host.frame(0.0).unwrap();
    let mut graph = host.capture_world_graph(root, Default::default()).unwrap();
    graph
        .nodes
        .iter_mut()
        .find(|node| node.world.metadata.symbolic_id == "camera-child")
        .unwrap()
        .world
        .entities
        .reverse();
    let bytes = graph.encode(71, Default::default()).unwrap();
    let descriptor = inspect_world_graph(&bytes, 71, Default::default()).unwrap();
    let loaded = host
        .load_world(
            &bytes,
            71,
            copy_names(&descriptor),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert!(host.root_output(loaded.root.id()).is_none());
    host.frame(0.0).unwrap();
    assert!(host.root_output(root).is_some());
    assert!(host.root_output(loaded.root.id()).is_none());
    let publication = host
        .publication(host.latest_publication(loaded.root.id()).unwrap())
        .unwrap();
    let attached = &publication.attachments[0];
    let restored_output = attached.output.unwrap();
    assert_ne!(restored_output, output);
    assert_ne!(restored_output.camera_entity(), Some(camera));
    assert_eq!(restored_output.world(), attached.child);
    assert!(
        loaded
            .created
            .values()
            .any(|world| *world == attached.child)
    );
    assert!(host.attached_publication(attached).is_some());

    let mut invalid = WorldGraphSnapshot::decode(&bytes, 71, Default::default()).unwrap();
    let non_camera = invalid
        .nodes
        .iter()
        .flat_map(|node| &node.world.entities)
        .find(|entity| {
            entity
                .components
                .iter()
                .any(|value| matches!(value, ComponentValue::Scalar(_)))
        })
        .unwrap()
        .persistent_id;
    let before = host.list_worlds();
    for node in &mut invalid.nodes {
        for reference in &mut node.references {
            if let WorldSerializedReferenceValue::Output(output) = &mut reference.value {
                output.entity = Some(non_camera);
            }
        }
    }
    let invalid = invalid.encode(71, Default::default()).unwrap();
    let mut options = copy_names(&descriptor);
    for name in options.world_names.values_mut() {
        name.push_str("-invalid");
    }
    assert!(
        host.load_world(
            &invalid,
            71,
            options,
            Default::default(),
            Default::default()
        )
        .is_err()
    );
    assert_eq!(host.list_worlds(), before);

    apply(
        &mut host,
        child,
        vec![
            Command::RemoveComponent {
                entity: EntityRef::Handle(camera),
                component: ComponentValue::CAMERA,
            },
            Command::insert_value(
                EntityRef::Handle(camera),
                ComponentValue::Camera(Camera::default()),
            ),
        ],
    );
    assert!(host.save_world(root, 71, Default::default()).is_err());
}

#[test]
fn descriptor_ids_are_explicit_and_need_not_match_node_order() {
    let (mut host, root, _) = fixture();
    let mut graph = host.capture_world_graph(root, Default::default()).unwrap();
    let mapping: BTreeMap<_, _> = graph
        .nodes
        .iter()
        .map(|node| (node.id, WorldGraphNodeId(101 + node.id.0 * 11)))
        .collect();
    graph.root = mapping[&graph.root];
    for node in &mut graph.nodes {
        node.id = mapping[&node.id];
        for reference in &mut node.references {
            match &mut reference.value {
                WorldSerializedReferenceValue::World(world) => *world = mapping[world],
                WorldSerializedReferenceValue::Output(output) => {
                    output.world = mapping[&output.world]
                }
            }
        }
    }
    graph.nodes.reverse();
    let bytes = graph.encode(71, Default::default()).unwrap();
    let descriptor = inspect_world_graph(&bytes, 71, Default::default()).unwrap();
    assert_ne!(descriptor.root, descriptor.nodes[0].id);
    let loaded = host
        .load_world(
            &bytes,
            71,
            copy_names(&descriptor),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    for node in descriptor.nodes {
        assert_eq!(
            host.world_mut(loaded.created[&node.id].id())
                .unwrap()
                .metadata()
                .symbolic_id,
            format!("copy-{}", node.metadata.symbolic_id)
        );
    }
    assert_eq!(loaded.created[&descriptor.root], loaded.root);
}

#[test]
fn late_system_restore_failure_tears_down_all_private_worlds_without_publishing_any() {
    let probe = Arc::new(DurableProbe::default());
    let mut factories = compiled_system_factories();
    factories.push(Arc::new(DurableFactory(probe.clone())));
    let mut host = crate::support::task_scheduler::with_factories(factories).unwrap();
    let durable = SystemId("fixture.graph-durable");
    let root = named(
        &mut host,
        "durable-root",
        [select(&[ATTACHMENTS]), vec![durable]].concat(),
    );
    let child = named(
        &mut host,
        "durable-child",
        [select(&[CONSTRAINTS]), vec![durable]].concat(),
    );
    let child_ref = host.world_ref(child).unwrap();
    entity(
        &mut host,
        root,
        vec![ComponentValue::WorldAttachment(WorldAttachment::spatial(
            child_ref,
        ))],
    );
    entity(
        &mut host,
        child,
        vec![ComponentValue::Scalar(Scalar {
            value: 4.0,
        })],
    );
    host.frame(0.0).unwrap();
    let publication = host.latest_publication(root).unwrap();
    let token = host.publication(publication).unwrap().attachments[0]
        .token
        .clone();
    let graph = host.capture_world_graph(root, Default::default()).unwrap();
    let bytes = graph.encode(71, Default::default()).unwrap();
    let descriptor = inspect_world_graph(&bytes, 71, Default::default()).unwrap();
    let before = host.list_worlds();
    probe.reject_second.store(true, Ordering::SeqCst);
    let failure = host
        .load_world(
            &bytes,
            71,
            copy_names(&descriptor),
            Default::default(),
            Default::default(),
        )
        .unwrap_err();
    assert!(
        failure
            .to_string()
            .contains("Rejected after graph components")
    );
    assert_eq!(probe.loads.load(Ordering::SeqCst), 2);
    assert_eq!(probe.teardowns.load(Ordering::SeqCst), 2);
    assert_eq!(host.list_worlds(), before);
    assert_eq!(host.latest_publication(root), Some(publication));
    assert_eq!(
        host.attachment_retirement(&token),
        Ok(WorldAttachmentRetirement::Pending)
    );
    assert_eq!(
        host.capture_world_graph(root, Default::default()).unwrap(),
        graph
    );
    probe.reject_second.store(false, Ordering::SeqCst);
    let loaded = host
        .load_world(
            &bytes,
            71,
            copy_names(&descriptor),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    assert_eq!(loaded.created.len(), 2);
    assert_eq!(probe.loads.load(Ordering::SeqCst), 4);
    assert_eq!(host.world_ids().len(), 4);
}

#[test]
fn real_canvas_graph_restores_selected_outputs_density_paint_and_sparse_systems() {
    use ipp_core::components::FlatSurface;
    use ipp_core::systems::canvas::{
        CanvasBox, CanvasPaintEntry, CanvasPrimitive, CanvasPublication, CanvasStyle, CanvasSystem,
    };

    fn paint(host: &HostRuntime, output: OutputRef) -> CanvasPublication {
        host.output(
            host.latest_publication(output.world().id()).unwrap(),
            output,
        )
        .unwrap()
        .data::<CanvasPublication>()
        .unwrap()
        .clone()
    }

    fn place(host: &mut HostRuntime, world: WorldId, entity: EntityId, parent: EntityId) {
        apply(
            host,
            world,
            vec![Command::PlaceEntity {
                entity: EntityRef::Handle(entity),
                placement: EntityPlacementRef {
                    parent: Some(EntityRef::Handle(parent)),
                    before: None,
                },
            }],
        );
    }

    let mut host = crate::support::task_scheduler::host();
    let mut worlds = Vec::new();
    for (name, systems, canvas) in [
        (
            "canvas-parent",
            select(&[ATTACHMENTS, CANVAS, SURFACE]),
            CanvasState {
                extent: [300.0, 200.0],
                units_per_metre: 100.0,
            },
        ),
        (
            "canvas-child",
            vec![CanvasSystem::ID],
            CanvasState {
                extent: [90.0, 70.0],
                units_per_metre: 80.0,
            },
        ),
    ] {
        worlds.push(
            host.create_world_with_options(
                Default::default(),
                WorldCreateOptions {
                    symbolic_id: name.into(),
                    canvas: Some(canvas),
                    ..WorldCreateOptions::new(systems)
                },
            )
            .unwrap(),
        );
    }
    let parent = worlds[0];
    let child = worlds[1];
    let parent_canvas = entity(&mut host, parent, vec![]);
    let child_canvas = entity(&mut host, child, vec![]);
    let parent_output = OutputRef::canvas(host.world_ref(parent).unwrap());
    let child_output = OutputRef::canvas(host.world_ref(child).unwrap());
    for horizontal in [7.0, 31.0] {
        let shape = entity(
            &mut host,
            child,
            vec![
                ComponentValue::CanvasStyle(CanvasStyle {
                    x: horizontal,
                    y: 9.0,
                    ..Default::default()
                }),
                ComponentValue::CanvasBox(CanvasBox {
                    width: 20.0,
                    height: 10.0,
                    ..Default::default()
                }),
            ],
        );
        place(&mut host, child, shape, child_canvas);
    }
    let surface = FlatSurface {
        width: 2.0,
        height: 0.5,
        ..Default::default()
    };
    let anchor = entity(
        &mut host,
        parent,
        vec![
            ComponentValue::CanvasStyle(CanvasStyle {
                x: 12.0,
                y: 18.0,
                ..Default::default()
            }),
            ComponentValue::FlatSurface(surface),
            ComponentValue::WorldAttachment(WorldAttachment::surface(child_output)),
        ],
    );
    place(&mut host, parent, anchor, parent_canvas);
    host.set_root_output(
        parent_output,
        WorldViewport {
            width: 640,
            height: 480,
            device_pixel_ratio: 2.0,
        },
    )
    .unwrap();
    let frame = host.frame(0.0).unwrap();
    assert!(frame.worlds.values().all(Result::is_ok));
    assert!(frame.publication_errors.is_empty());
    assert_eq!(paint(&host, parent_output).logical_extent, [320.0, 240.0]);
    assert_eq!(paint(&host, child_output).logical_extent, [160.0, 40.0]);

    let mut graph = host
        .capture_world_graph(parent, Default::default())
        .unwrap();
    for node in &mut graph.nodes {
        node.world.entities.reverse();
    }
    let bytes = graph.encode(71, Default::default()).unwrap();
    let descriptor = inspect_world_graph(&bytes, 71, Default::default()).unwrap();
    assert_eq!(descriptor.nodes.len(), 2);
    let child_node = descriptor
        .nodes
        .iter()
        .find(|node| node.metadata.symbolic_id == "canvas-child")
        .unwrap()
        .id;
    // A SurfaceCanvas attachment names only its child World; the presented
    // canvas follows from that World.
    let references: Vec<_> = graph
        .nodes
        .iter()
        .flat_map(|node| &node.references)
        .collect();
    assert_eq!(references.len(), 1);
    assert_eq!(
        references[0].value,
        WorldSerializedReferenceValue::World(child_node)
    );
    // A later density change of the original does not reach the saved graph.
    host.world_mut(child)
        .unwrap()
        .enqueue_canvas_state_update(CanvasStateUpdate {
            extent: None,
            units_per_metre: Some(160.0),
        })
        .unwrap();

    let loaded = host
        .load_world(
            &bytes,
            71,
            copy_names(&descriptor),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let restored_child = loaded.created[&child_node];
    for node in &graph.nodes {
        let manifest = host.world_manifest(loaded.created[&node.id].id()).unwrap();
        assert_eq!(
            manifest
                .systems()
                .iter()
                .map(|system| system.0.to_owned())
                .collect::<Vec<_>>(),
            node.world.selected_systems
        );
        assert!(manifest.supports_operation(WorldOperation::Canvas));
        if node.id == child_node {
            assert!(!manifest.supports_operation(WorldOperation::Camera));
            assert!(!manifest.supports_operation(WorldOperation::Geometry));
            assert!(!manifest.supports_component(ComponentValue::TRANSFORM));
        }
    }
    let frame = host.frame(0.0).unwrap();
    assert!(frame.worlds.values().all(Result::is_ok));
    assert!(frame.publication_errors.is_empty());
    assert_eq!(host.root_output(parent).unwrap().0, parent_output);
    assert!(host.root_output(loaded.root.id()).is_none());
    let publication = host
        .publication(host.latest_publication(loaded.root.id()).unwrap())
        .unwrap();
    assert_eq!(publication.attachments.len(), 1);
    let edge = &publication.attachments[0];
    let restored_output = edge.output.unwrap();
    let restored_parent = edge.placement_output.unwrap();
    assert_eq!(restored_output.world(), restored_child);
    assert_eq!(restored_output.kind(), OutputKind::Canvas);
    assert_ne!(restored_output, child_output);
    assert_eq!(restored_parent.world(), loaded.root);
    assert!(host.attached_publication(edge).is_some());
    assert_eq!(edge.surface_extent, Some([2.0, 0.5]));
    let parent_paint = paint(&host, restored_parent);
    assert_eq!(parent_paint.logical_extent, [300.0, 200.0]);
    let CanvasPaintEntry::Attachment(slot) = parent_paint.entries[0].as_ref() else {
        panic!("missing restored Canvas slot")
    };
    assert_eq!(slot.token, edge.token);
    assert_eq!(slot.anchor, edge.anchor);
    assert_eq!(slot.to_canvas([-1.0, 0.25]), [12.0, 18.0]);
    assert_eq!(
        edge.placement,
        slot.parent_affine(parent_paint.logical_extent, parent_paint.units_per_metre)
    );
    let child_paint = paint(&host, restored_output);
    assert_eq!(child_paint.logical_extent, [160.0, 40.0]);
    assert_eq!(child_paint.units_per_metre, 80.0);
    assert_eq!(paint(&host, child_output).logical_extent, [320.0, 80.0]);
    let restored_root = support::top_level_root(&mut host, restored_child.id());
    assert_eq!(child_paint.entries.len(), 2);
    for (entry, horizontal) in child_paint.entries.iter().zip([7.0, 31.0]) {
        let CanvasPaintEntry::Primitive {
            primitive,
            ..
        } = entry.as_ref()
        else {
            panic!("missing restored raw primitive")
        };
        let CanvasPrimitive::Box {
            style,
            size,
            ..
        } = primitive
        else {
            panic!("wrong raw primitive kind")
        };
        assert_eq!(style.position, [horizontal, 9.0]);
        assert_eq!(*size, [20.0, 10.0]);
        assert_eq!(style.identity.target.component, ComponentValue::CANVAS_BOX);
        assert_eq!(
            host.world_mut(restored_child.id())
                .unwrap()
                .entity_link(style.identity.target.entity)
                .unwrap()
                .parent,
            Some(restored_root)
        );
    }

    let before = host.list_worlds();
    let mut unselected = graph.clone();
    let unselected_world = &mut unselected
        .nodes
        .iter_mut()
        .find(|node| node.id == child_node)
        .unwrap()
        .world;
    unselected_world.selected_systems.clear();
    unselected_world.systems.clear();
    // A SurfaceCanvas attachment carrying an output reference is not a valid graph.
    let mut canvas_output = graph;
    let attachment = canvas_output
        .nodes
        .iter()
        .flat_map(|node| &node.references)
        .next()
        .unwrap()
        .clone();
    canvas_output
        .nodes
        .iter_mut()
        .find(|node| node.references.contains(&attachment))
        .unwrap()
        .references
        .push(WorldSerializedReference {
            field: std::mem::offset_of!(WorldAttachment, output) as u32,
            value: WorldSerializedReferenceValue::Output(WorldSerializedOutput {
                world: child_node,
                kind: OutputKind::Canvas,
                entity: None,
            }),
            ..attachment
        });
    assert_eq!(
        canvas_output.encode(71, Default::default()).unwrap_err(),
        "Invalid serialized attachment output"
    );
    {
        let invalid = unselected.encode(71, Default::default()).unwrap();
        let mut options = copy_names(&descriptor);
        for name in options.world_names.values_mut() {
            name.push_str("-unsupported");
        }
        assert!(
            host.load_world(
                &invalid,
                71,
                options,
                Default::default(),
                Default::default()
            )
            .is_err()
        );
        assert_eq!(host.list_worlds(), before);
    }
}

#[test]
fn curved_provider_parameters_round_trip_and_conflicting_providers_fail_graph_validation() {
    let mut host = support::task_scheduler::host();
    let root = named(&mut host, "curved-providers", select(&[SURFACE]));
    let cylinder = CylinderSurface {
        width: 4.0,
        height: 2.0,
        curvature: -0.7,
        layer_spacing: 0.1,
    };
    let sphere = SphereSurface {
        width: 3.0,
        height: 2.0,
        curvature: 0.5,
        layer_spacing: -0.2,
    };
    entity(
        &mut host,
        root,
        vec![ComponentValue::CylinderSurface(cylinder.clone())],
    );
    entity(
        &mut host,
        root,
        vec![ComponentValue::SphereSurface(sphere.clone())],
    );
    let graph = host.capture_world_graph(root, Default::default()).unwrap();
    let bytes = graph.encode(71, Default::default()).unwrap();
    let descriptor = inspect_world_graph(&bytes, 71, Default::default()).unwrap();
    let loaded = host
        .load_world(
            &bytes,
            71,
            copy_names(&descriptor),
            Default::default(),
            Default::default(),
        )
        .unwrap();
    host.frame(0.0).unwrap();
    let restored = host
        .capture_world_graph(loaded.root.id(), Default::default())
        .unwrap();
    let values: Vec<_> = restored
        .nodes
        .iter()
        .flat_map(|node| &node.world.entities)
        .flat_map(|entity| &entity.components)
        .collect();
    assert!(values.contains(&&ComponentValue::CylinderSurface(cylinder)));
    assert!(values.contains(&&ComponentValue::SphereSurface(sphere)));
    let world = host.world_mut(loaded.root.id()).unwrap();
    for entity in world
        .entities()
        .iter()
        .map(|entity| entity.id)
        .collect::<Vec<_>>()
    {
        assert!(world.surface(entity).is_some());
    }
    drop(world);

    let mut invalid = graph;
    invalid.nodes[0].world.entities[0]
        .components
        .push(ComponentValue::FlatSurface(FlatSurface::default()));
    assert_eq!(
        invalid.encode(71, Default::default()).unwrap_err(),
        "Multiple serialized Surface providers"
    );
}
