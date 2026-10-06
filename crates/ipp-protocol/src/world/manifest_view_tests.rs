use super::*;
use ipp_core::Batch;

fn viewport() -> ipp_core::WorldViewport {
    ipp_core::WorldViewport {
        width: 640,
        height: 480,
        device_pixel_ratio: 1.25,
    }
}

fn view() -> ipp_core::ViewDescriptor {
    let mut host = ipp_core::HostRuntime::new();
    let world = host
        .create_world(
            Default::default(),
            &[
                ipp_core::systems::animation::AnimationSystem::ID,
                ipp_core::systems::asset_dependencies::AssetDependencySystem::ID,
                ipp_core::systems::hierarchy::HierarchySystem::ID,
                ipp_core::systems::look_at::LookAtSystem::ID,
                ipp_core::systems::hierarchy::FinalPropagationSystem::ID,
                ipp_core::systems::geometry::GeometrySystem::ID,
                ipp_core::systems::camera::CameraSystem::ID,
            ],
        )
        .unwrap();
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations: vec![
                Command::Create {
                    alias: 1,
                    metadata: Default::default(),
                    adopt: false,
                },
                Command::insert_value(
                    EntityRef::Alias(1),
                    ComponentValue::Camera(Default::default()),
                ),
            ],
        })
        .unwrap();

    let entity = host
        .frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
        .result
        .unwrap()[0]
        .1;

    let output = host
        .bind_output(
            host.world_ref(world).unwrap(),
            entity,
            ipp_core::OutputKind::Camera,
        )
        .unwrap();
    host.set_root_output(output, viewport()).unwrap();
    host.frame(0.0).unwrap();
    host.resolve_view(ipp_core::ViewQueryTarget::RootView {
        output,
        expected_viewport: viewport(),
    })
    .unwrap()
}

fn world(value: ipp_core::WorldRef) -> ManifestValue {
    manifest_layout(
        "world-reference",
        [
            ("id", ManifestValue::U64(value.id().0)),
            ("incarnation", ManifestValue::U64(value.incarnation())),
        ],
    )
}

/// The Camera entity and component lifetime a camera output selects.
fn camera(value: ipp_core::OutputRef) -> (ipp_core::EntityId, u64) {
    let ipp_core::OutputTarget::Camera {
        entity,
        incarnation,
    } = value.target()
    else {
        panic!("camera output");
    };
    (entity, incarnation)
}

fn output(value: ipp_core::OutputRef) -> ManifestValue {
    manifest_layout(
        "output-reference",
        [
            ("world", world(value.world())),
            (
                "target",
                manifest_layout(
                    "output-target-camera",
                    [
                        ("tag", ManifestValue::Tag("OUTPUT_TARGET_CAMERA")),
                        ("entity", ManifestValue::U64(camera(value).0.to_bits())),
                        ("incarnation", ManifestValue::U64(camera(value).1)),
                    ],
                ),
            ),
        ],
    )
}

fn dimensions() -> ManifestValue {
    manifest_layout(
        "view-viewport",
        [
            ("width", ManifestValue::U32(640)),
            ("height", ManifestValue::U32(480)),
            ("device_pixel_ratio", ManifestValue::F64(1.25)),
        ],
    )
}

fn publication(value: ipp_core::WorldPublicationId) -> ManifestValue {
    let (host, revision) = value.identity();
    manifest_layout(
        "publication-reference",
        [
            ("host", ManifestValue::U64(host)),
            ("revision", ManifestValue::U64(revision)),
        ],
    )
}

fn descriptor(view: ipp_core::ViewDescriptor) -> ManifestValue {
    manifest_layout(
        "view-descriptor",
        [
            ("output", output(view.output)),
            ("publication", publication(view.publication)),
            ("viewport", dimensions()),
        ],
    )
}

pub(super) fn requests(covered: &mut BTreeSet<&'static str>) {
    let view = view();
    let (host, revision) = view.publication.identity();
    for historical in [false, true] {
        let target = if historical {
            view_queries::ViewQueryTarget::PublicationView {
                output: view.output.into(),
                host,
                revision,
                viewport: viewport(),
            }
        } else {
            view_queries::ViewQueryTarget::RootView {
                output: view.output.into(),
                expected_viewport: viewport(),
            }
        };
        for projection in [false, true] {
            let encoded_view = if historical {
                manifest_layout(
                    "view-publication",
                    [
                        ("tag", ManifestValue::Tag("VIEW_PUBLICATION")),
                        ("output", output(view.output)),
                        ("publication", publication(view.publication)),
                        ("viewport", dimensions()),
                    ],
                )
            } else {
                manifest_layout(
                    "view-root",
                    [
                        ("tag", ManifestValue::Tag("VIEW_ROOT")),
                        ("output", output(view.output)),
                        ("expected_viewport", dimensions()),
                    ],
                )
            };
            let plane = ipp_core::WorldPlane {
                point: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
            };
            let mut fields = vec![
                ("session", ManifestValue::U64(7)),
                ("request_id", ManifestValue::U64(21)),
                (
                    "tag",
                    ManifestValue::Tag(if projection {
                        "REQUEST_CAMERA_PROJECT"
                    } else {
                        "REQUEST_GEOMETRY_PICK"
                    }),
                ),
                ("view", encoded_view),
                ("x", ManifestValue::F32(0.25)),
                ("y", ManifestValue::F32(0.75)),
            ];
            fields.push(if projection {
                ("plane", manifest_plane(plane))
            } else {
                ("include_view_plane", ManifestValue::Bool(true))
            });
            let fixture = ManifestFixture::new(
                if projection {
                    "request-camera-project"
                } else {
                    "request-geometry-pick"
                },
                fields,
            );
            let decoded = decode_request(&encode_manifest_fixture(&fixture, covered), 7).unwrap();
            let expected = if projection {
                RequestBody::CameraProjectQuery(view_queries::CameraProjectQuery {
                    view: target,
                    x: 0.25,
                    y: 0.75,
                    plane,
                })
            } else {
                RequestBody::GeometryPickQuery(view_queries::GeometryPickQuery {
                    view: target,
                    x: 0.25,
                    y: 0.75,
                    include_view_plane: true,
                })
            };
            assert_eq!(decoded.body, expected);
        }
    }
    // A World canvas output names no producer entity.
    let request = ManifestFixture::new(
        "request-geometry-pick",
        [
            ("session", ManifestValue::U64(7)),
            ("request_id", ManifestValue::U64(22)),
            ("tag", ManifestValue::Tag("REQUEST_GEOMETRY_PICK")),
            (
                "view",
                manifest_layout(
                    "view-root",
                    [
                        ("tag", ManifestValue::Tag("VIEW_ROOT")),
                        (
                            "output",
                            manifest_layout(
                                "output-reference",
                                [
                                    ("world", world(view.output.world())),
                                    (
                                        "target",
                                        manifest_layout(
                                            "output-target-canvas",
                                            [("tag", ManifestValue::Tag("OUTPUT_TARGET_CANVAS"))],
                                        ),
                                    ),
                                ],
                            ),
                        ),
                        ("expected_viewport", dimensions()),
                    ],
                ),
            ),
            ("x", ManifestValue::F32(0.5)),
            ("y", ManifestValue::F32(0.5)),
            ("include_view_plane", ManifestValue::Bool(false)),
        ],
    );
    let RequestBody::GeometryPickQuery(decoded) =
        decode_request(&encode_manifest_fixture(&request, covered), 7)
            .unwrap()
            .body
    else {
        panic!("pick query");
    };
    assert_eq!(
        decoded.view,
        view_queries::ViewQueryTarget::RootView {
            output: crate::references::OutputReference::canvas(view.output.world().into()),
            expected_viewport: viewport()
        }
    );
}

pub(super) fn responses(covered: &mut BTreeSet<&'static str>) {
    let view = view();
    let plane = ipp_core::WorldPlane {
        point: [1.0, 2.0, 3.0],
        normal: [0.0, 0.0, -1.0],
    };
    for hit_plane in [None, Some(plane)] {
        let hit = ipp_core::ViewPickHit {
            identity: ipp_core::PublishedSceneHit {
                publication: view.publication,
                world: view.output.world(),
                entity: camera(view.output).0,
                incarnation: 15,
                component: ipp_core::ComponentValue::PICKING_GEOMETRY,
                row: None,
                hit: ipp_core::systems::geometry::GeometryRayHit {
                    distance: 4.0,
                    part: 8,
                },
                path: vec![(view.output.world(), camera(view.output).0)],
            },
            position: [1.0, 2.0, 3.0],
            view_plane: hit_plane,
        };
        for result in [
            Ok(Some(hit.clone())),
            Ok(None),
            Err(ipp_core::ErrorReason::InvalidEntity),
        ] {
            let encoded = match &result {
                Ok(Some(hit)) => manifest_layout(
                    "pick-result-hit",
                    vec![
                        ("tag", ManifestValue::Tag("PICK_OUTCOME_HIT")),
                        ("view", descriptor(view)),
                        ("world", world(view.output.world())),
                        ("publication", publication(view.publication)),
                        (
                            "entity",
                            ManifestValue::U64(camera(view.output).0.to_bits()),
                        ),
                        ("incarnation", ManifestValue::U64(15)),
                        (
                            "component",
                            ManifestValue::U16(ipp_core::ComponentValue::PICKING_GEOMETRY),
                        ),
                        ("plot_row", ManifestValue::None),
                        ("position_x", ManifestValue::F32(1.0)),
                        ("position_y", ManifestValue::F32(2.0)),
                        ("position_z", ManifestValue::F32(3.0)),
                        ("distance", ManifestValue::F64(4.0)),
                        ("part", ManifestValue::U32(8)),
                        (
                            "path",
                            ManifestValue::List(vec![manifest_layout(
                                "view-path-entry",
                                [
                                    ("world", world(view.output.world())),
                                    (
                                        "anchor",
                                        ManifestValue::U64(camera(view.output).0.to_bits()),
                                    ),
                                ],
                            )]),
                        ),
                        (
                            "view_plane",
                            hit.view_plane.map_or(ManifestValue::None, |plane| {
                                ManifestValue::Some(Box::new(manifest_plane(plane)))
                            }),
                        ),
                    ],
                ),
                Ok(None) => manifest_layout(
                    "pick-result-miss",
                    [
                        ("tag", ManifestValue::Tag("PICK_OUTCOME_MISS")),
                        ("view", descriptor(view)),
                    ],
                ),
                Err(_) => manifest_layout(
                    "pick-result-failure",
                    [
                        ("tag", ManifestValue::Tag("PICK_OUTCOME_FAILURE")),
                        ("reason", ManifestValue::String("InvalidEntity".into())),
                    ],
                ),
            };
            assert_manifest_response(
                Response {
                    session: 7,
                    request_id: 21,
                    tick: 3,
                    body: ResponseBody::GeometryPickResultEvent(view_queries::ViewQueryOutcome {
                        request_id: 21,
                        tick: 3,
                        result: result.map(|hit| (view, hit)),
                    }),
                },
                ManifestFixture::new(
                    "response-geometry-pick",
                    [
                        ("session", ManifestValue::U64(7)),
                        ("request_id", ManifestValue::U64(21)),
                        ("tick", ManifestValue::U64(3)),
                        ("tag", ManifestValue::Tag("RESPONSE_GEOMETRY_PICK")),
                        ("result", encoded),
                    ],
                ),
                covered,
            );
        }
    }
    for result in [
        Ok(Some([1.0, 2.0, 3.0])),
        Ok(None),
        Err(ipp_core::ErrorReason::InvalidViewport),
    ] {
        let position = if let Ok(Some([horizontal, vertical, depth])) = result {
            ManifestValue::Some(Box::new(manifest_layout(
                "world-point",
                [
                    ("x", ManifestValue::F32(horizontal)),
                    ("y", ManifestValue::F32(vertical)),
                    ("z", ManifestValue::F32(depth)),
                ],
            )))
        } else {
            ManifestValue::None
        };
        assert_manifest_response(
            Response {
                session: 7,
                request_id: 22,
                tick: 3,
                body: ResponseBody::CameraProjectResultEvent(view_queries::ViewQueryOutcome {
                    request_id: 22,
                    tick: 3,
                    result: result.map(|position| (view, position)),
                }),
            },
            ManifestFixture::new(
                "response-camera-project",
                [
                    ("session", ManifestValue::U64(7)),
                    ("request_id", ManifestValue::U64(22)),
                    ("tick", ManifestValue::U64(3)),
                    ("tag", ManifestValue::Tag("RESPONSE_CAMERA_PROJECT")),
                    (
                        "view",
                        if result.is_ok() {
                            ManifestValue::Some(Box::new(descriptor(view)))
                        } else {
                            ManifestValue::None
                        },
                    ),
                    ("ok", ManifestValue::Bool(result.is_ok())),
                    ("position", position),
                    (
                        "error",
                        if result.is_err() {
                            ManifestValue::Some(Box::new(ManifestValue::String(
                                "InvalidViewport".into(),
                            )))
                        } else {
                            ManifestValue::None
                        },
                    ),
                ],
            ),
            covered,
        );
    }
}
