//! Ordinary skin motion through real Host mutation, asset demand and retained Canvas output.

mod support;

use ipp_core::components::rows::Rows;
use ipp_core::components::{
    GuiBehavior, GuiButton, GuiCheckbox, GuiLayout, GuiSlider, GuiTextInput,
};
use ipp_core::services::asset_management::{AssetKey, AssetSource};
use ipp_core::services::gui_input::{
    GuiDeliveryError, GuiDeliveryPermit, GuiDeliveryTerminal, GuiInputService, GuiInputSession,
};
use ipp_core::systems::animation::*;
use ipp_core::systems::canvas::{
    CanvasPaintEntry, CanvasPrimitive, CanvasPublication, CanvasShapeFill,
};
use ipp_core::systems::gui::GuiPrimitivePart;
use ipp_core::systems::gui::local::{GuiLocalAction, GuiLocalCommand, GuiLocalEffect};
use ipp_core::systems::gui::motion::{GuiMotionPart, GuiSkinMotionStatus, GuiThemeMotion};
use ipp_core::systems::gui::presentation::{GuiPaintPart, GuiSkin, GuiTheme};
use ipp_core::systems::gui::{GuiPartId, GuiPartVariant, GuiSkinState, GuiSystem};
use ipp_core::*;
use std::sync::Arc;
use support::CanvasTestHost;
use support::gui_panel::{ControlRead, ControlValue, read_control};
use support::selection::{CAMERA, GUI_LAYOUT, RENDER, select};

const SOURCE: &str = "fixture:///ordinary-motion";
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

struct Applied;

impl GuiDeliveryPermit for Applied {
    fn prepare(&mut self, _: Option<&GuiLocalEffect>) -> Result<(), GuiDeliveryError> {
        Ok(())
    }

    fn settle(self: Box<Self>, terminal: GuiDeliveryTerminal) {
        assert!(
            matches!(
                terminal,
                GuiDeliveryTerminal::Applied(_) | GuiDeliveryTerminal::Written { .. }
            ),
            "{terminal:?}"
        );
    }
}

struct Fixture {
    host: HostRuntime,
    world: WorldId,
    theme: EntityId,
    control: EntityId,
    /// Top-level root holding the control.
    canvas: EntityId,
    output: OutputRef,
    input: GuiInputService,
    session: GuiInputSession,
    request: u64,
}

fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
    host.world_mut(world)
        .unwrap()
        .enqueue(Batch {
            id: 1,
            operations,
        })
        .unwrap();

    host.frame(0.0)
        .unwrap()
        .worlds
        .remove(&world)
        .unwrap()
        .unwrap()
        .outcomes
        .remove(0)
}

fn create(
    host: &mut HostRuntime,
    world: WorldId,
    components: Vec<ComponentValue>,
    parent: Option<EntityId>,
) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        components
            .into_iter()
            .map(|value| Command::insert_value(EntityRef::Alias(1), value)),
    );
    if let Some(parent) = parent {
        operations.push(Command::PlaceEntity {
            entity: EntityRef::Alias(1),
            placement: EntityPlacementRef {
                parent: Some(EntityRef::Handle(parent)),
                before: None,
            },
        });
    }
    apply(host, world, operations).result.unwrap()[0].1
}

fn source(uri: &str) -> AssetSource {
    AssetSource {
        kind: ANIMATION_TYPE,
        uri: uri.into(),
        variant: 0,
    }
}

fn declarations(uri: &str) -> (GuiTheme, GuiThemeMotion) {
    let mut appearance = Rows::default();
    let mut motion = Rows::default();
    for (part, color, time) in [
        (GuiPartId::base(GuiPrimitivePart::Background), RED, 0.0),
        (
            GuiPartId::state(GuiPrimitivePart::Background, GuiSkinState::Disabled),
            BLUE,
            1.0,
        ),
        (
            GuiPartId::variant(
                GuiPrimitivePart::Background,
                GuiSkinState::Idle,
                GuiPartVariant::Checked,
            ),
            BLUE,
            1.0,
        ),
    ] {
        appearance
            .push(GuiPaintPart {
                color: Some(color),
                opacity: Some(1.0),
                scale: Some([1.0; 2]),
                ..GuiPaintPart::keyed(part).unwrap()
            })
            .unwrap();
        motion
            .push(GuiMotionPart {
                part: part.index().unwrap(),
                source: Some(source(uri)),
                duration: Some(1.0),
                easing: Some(0),
                track: Some(0),
                time: Some(time),
            })
            .unwrap();
    }
    (
        GuiTheme {
            parts: appearance,
        },
        GuiThemeMotion {
            parts: motion,
        },
    )
}

fn clip() -> AnimationClip {
    let track = |name: &str, first: DynamicValue, last: DynamicValue| AnimationTrack {
        target: AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::CUSTOM_MATERIAL,
            name: name.into(),
        },
        keys: vec![
            AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::Field(components::schema::FieldValue::Dynamic(first)),
                interpolation: AnimationInterpolation::Linear,
            },
            AnimationKeyframe {
                time: 1.0,
                value: AnimationValue::Field(components::schema::FieldValue::Dynamic(last)),
                interpolation: AnimationInterpolation::Step,
            },
        ],
    };
    AnimationClip::new(
        1.0,
        vec![
            track("color", DynamicValue::Vec4(RED), DynamicValue::Vec4(BLUE)),
            track("opacity", DynamicValue::F32(1.0), DynamicValue::F32(1.0)),
            track(
                "scale",
                DynamicValue::Vec2([1.0; 2]),
                DynamicValue::Vec2([1.0; 2]),
            ),
        ],
    )
    .unwrap()
}

impl Fixture {
    fn new(control: ComponentValue) -> Self {
        let mut host = HostRuntime::new();
        host.register_stream_resource_provider("fixture").unwrap();
        let world = host
            .create_world(Default::default(), &select(&[CAMERA, GUI_LAYOUT, RENDER]))
            .unwrap();
        let canvas = create(&mut host, world, vec![], None);
        let (appearance, motion) = declarations(SOURCE);
        let theme = create(
            &mut host,
            world,
            vec![
                ComponentValue::GuiTheme(appearance),
                ComponentValue::GuiThemeMotion(motion),
            ],
            None,
        );
        let control = create(
            &mut host,
            world,
            vec![
                control,
                ComponentValue::GuiBehavior(GuiBehavior::default()),
                ComponentValue::GuiLayout(GuiLayout {
                    width: 40.0,
                    height: 20.0,
                    ..Default::default()
                }),
                ComponentValue::GuiSkin(GuiSkin {
                    theme,
                    ..Default::default()
                }),
            ],
            Some(canvas),
        );
        let world_ref = host.world_ref(world).unwrap();
        let output = host.canvas_output(world_ref, [100.0, 100.0], 1.0);
        let input = GuiInputService::default();
        let session = input.open_session().unwrap();
        let mut fixture = Self {
            host,
            world,
            theme,
            control,
            canvas,
            output,
            input,
            session,
            request: 0,
        };
        fixture.frame(0.0);
        fixture.load(SOURCE);
        fixture
    }

    fn frame(&mut self, dt: f64) {
        let result = self.host.frame(dt).unwrap();
        assert!(
            result.worlds.values().all(Result::is_ok),
            "{:?}",
            result.worlds
        );
        assert!(
            result.publication_errors.is_empty(),
            "{:?}",
            result.publication_errors
        );
    }

    fn load(&mut self, uri: &str) {
        self.load_clip(uri, &clip());
    }

    fn load_clip(&mut self, uri: &str, clip: &AnimationClip) {
        for _ in 0..16 {
            let requests = self.host.take_resource_requests();
            if !requests.is_empty() {
                for request in requests {
                    assert_eq!(request.source, std::sync::Arc::<str>::from(uri));
                    self.host
                        .complete_resource(request.id, Ok(clip.encode()))
                        .unwrap();
                }
                for _ in 0..8 {
                    self.frame(0.0);
                }
                return;
            }
            self.frame(0.0);
        }
        panic!("ordinary component demand did not request {uri}");
    }

    fn publication(&self) -> CanvasPublication {
        self.host
            .publication(self.host.latest_publication(self.world).unwrap())
            .unwrap()
            .output(self.output)
            .unwrap()
            .data::<CanvasPublication>()
            .unwrap()
            .clone()
    }

    fn color(&self) -> [f32; 4] {
        self.publication()
            .entries
            .iter()
            .find_map(|entry| match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    primitive:
                        CanvasPrimitive::Box {
                            style,
                            fill: CanvasShapeFill::Solid(color),
                            ..
                        },
                    ..
                } if style.identity.target.entity == self.control => Some(*color),
                _ => None,
            })
            .unwrap()
    }

    fn status(&mut self) -> Option<GuiSkinMotionStatus> {
        self.host
            .world_mut(self.world)
            .unwrap()
            .inspect(self.control)
            .unwrap()
            .components
            .iter()
            .find_map(|component| match component {
                ComponentValue::GuiSkin(skin) => skin.runtime.status(GuiPrimitivePart::Background),
                _ => None,
            })
    }

    fn enabled(&mut self, enabled: bool) {
        apply(
            &mut self.host,
            self.world,
            vec![Command::SetField {
                entity: EntityRef::Handle(self.control),
                component: ComponentValue::GUI_BEHAVIOR,
                field: FieldWrite {
                    offset: std::mem::offset_of!(GuiBehavior, enabled) as u32,
                    value: FieldValue::Bool(enabled),
                },
            }],
        )
        .result
        .unwrap();
    }

    /// Queue one `GuiAction` command; the next frame applies it.
    fn action(&mut self, action: GuiLocalAction) {
        self.request += 1;
        let target = self.read(self.control).unwrap().target;
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue(Batch {
                id: 1 << 32 | self.request,
                operations: vec![Command::GuiAction {
                    target: GuiActionTarget {
                        entity: EntityRef::Handle(target.entity),
                        component: target.component,
                        incarnation: target.incarnation,
                    },
                    action,
                }],
            })
            .unwrap();
    }

    /// Read `entity` the way a client does, if it is a control.
    fn read(&mut self, entity: EntityId) -> Option<ControlRead> {
        read_control(&mut self.host, self.world, entity)
    }

    fn key(&self) -> AssetKey {
        self.host.asset_resources().find(&source(SOURCE)).unwrap()
    }
}

#[test]
fn four_controls_sample_once_before_paint_without_reflow_and_reuse_settled_chunks() {
    for control in [
        ComponentValue::GuiButton(GuiButton::default()),
        ComponentValue::GuiCheckbox(GuiCheckbox::default()),
        ComponentValue::GuiSlider(GuiSlider::default()),
        ComponentValue::GuiTextInput(GuiTextInput::default()),
    ] {
        let mut fixture = Fixture::new(control);
        assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
        assert_eq!(fixture.color(), RED);
        let measured = fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .gui_entity_layout_statistics()
            .unwrap()
            .total;
        fixture.enabled(false);
        fixture.frame(0.25);
        assert_eq!(fixture.color(), [0.75, 0.0, 0.25, 1.0]);
        fixture.frame(0.25);
        assert_eq!(fixture.color(), [0.5, 0.0, 0.5, 1.0]);
        assert_eq!(
            fixture
                .host
                .world_mut(fixture.world)
                .unwrap()
                .gui_entity_layout_statistics()
                .unwrap()
                .total,
            measured
        );
        fixture.frame(0.5);
        assert_eq!(fixture.color(), BLUE);
        let settled = fixture.publication();
        fixture.frame(1.0);
        let unchanged = fixture.publication();
        assert_eq!(settled.paint_revision, unchanged.paint_revision);
        assert!(Arc::ptr_eq(&settled.entries, &unchanged.entries));
    }
}

#[test]
fn ordered_semantic_toggle_retargets_from_actual_composite_without_extra_frame() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    assert_eq!(fixture.color(), [0.75, 0.0, 0.25, 1.0]);
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.5);
    assert_eq!(fixture.color(), [0.875, 0.0, 0.125, 1.0]);
    let snapshot = fixture.read(fixture.control).unwrap();
    assert_eq!(snapshot.value, ControlValue::Bool(false));
    assert!(!snapshot.focused);
}

#[test]
fn deleting_independent_theme_withdraws_active_settled_and_pending_motion() {
    for stage in ["active", "settled", "pending"] {
        for component in [
            None,
            Some(ComponentValue::GUI_THEME),
            Some(ComponentValue::GUI_THEME_MOTION),
        ] {
            let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
            fixture.action(GuiLocalAction::Toggle);
            fixture.frame(if stage == "settled" {
                1.0
            } else {
                0.25
            });
            if stage == "pending" {
                let (_, motion) = declarations("fixture:///theme-delete-pending");
                apply(
                    &mut fixture.host,
                    fixture.world,
                    vec![Command::insert_value(
                        EntityRef::Handle(fixture.theme),
                        ComponentValue::GuiThemeMotion(motion),
                    )],
                )
                .result
                .unwrap();
                fixture.frame(0.0);
                assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Pending));
            }
            let key = fixture.key();
            fixture
                .host
                .asset_resources_mut()
                .set_idle_resident_bytes_target(0);
            let command = match component {
                Some(component) => Command::RemoveComponent {
                    entity: EntityRef::Handle(fixture.theme),
                    component,
                },
                None => Command::Delete {
                    entity: EntityRef::Handle(fixture.theme),
                },
            };
            apply(&mut fixture.host, fixture.world, vec![command])
                .result
                .unwrap();
            assert_eq!(
                fixture.status(),
                None,
                "before evaluation: {stage} {component:?}"
            );
            fixture.frame(0.25);
            assert_eq!(fixture.status(), None, "{stage}");
            if component == Some(ComponentValue::GUI_THEME_MOTION) {
                assert_eq!(fixture.color(), BLUE);
            }
            fixture.host.flush_resource_lifecycle();
            if component != Some(ComponentValue::GUI_THEME) || stage == "pending" {
                assert!(
                    fixture
                        .host
                        .asset_resources()
                        .get_typed::<AnimationClip>(key)
                        .is_none(),
                    "{stage}"
                );
            }
        }
    }
}

#[test]
fn theme_deletion_releases_last_ready_motion_demand() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    let (_, motion) = declarations("fixture:///deleted-theme-pending");
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::insert_value(
            EntityRef::Handle(fixture.theme),
            ComponentValue::GuiThemeMotion(motion),
        )],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    let key = fixture.key();
    let track = Arc::downgrade(
        &fixture
            .host
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .unwrap()
            .tracks()[0],
    );
    fixture
        .host
        .asset_resources_mut()
        .set_idle_resident_bytes_target(0);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::Delete {
            entity: EntityRef::Handle(fixture.theme),
        }],
    )
    .result
    .unwrap();
    assert_eq!(fixture.status(), None);

    fixture.host.flush_resource_lifecycle();
    assert!(track.upgrade().is_none());
    assert!(
        fixture
            .host
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .is_none()
    );

    fixture.frame(0.25);
    assert_eq!(fixture.status(), None);
}

#[test]
fn never_ready_theme_deletion_cannot_reappear_on_late_readiness() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let uri = "fixture:///never-ready-theme";
    apply(
        &mut fixture.host,
        fixture.world,
        vec![
            Command::insert_value(
                EntityRef::Handle(fixture.control),
                ComponentValue::GuiSkin(GuiSkin {
                    theme: fixture.theme,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiThemeMotion(declarations(uri).1),
            ),
        ],
    )
    .result
    .unwrap();
    let mut requests = Vec::new();
    for _ in 0..16 {
        fixture.frame(0.0);
        requests = fixture.host.take_resource_requests();
        if !requests.is_empty() {
            break;
        }
    }
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].source, std::sync::Arc::<str>::from(uri));
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Pending));
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::Delete {
            entity: EntityRef::Handle(fixture.theme),
        }],
    )
    .result
    .unwrap();
    assert_eq!(fixture.status(), None);

    fixture
        .host
        .complete_resource(requests[0].id, Ok(clip().encode()))
        .unwrap();
    for _ in 0..8 {
        fixture.host.progress_evaluation_assets();
    }
    assert_eq!(fixture.status(), None);

    fixture.frame(0.0);
    assert_eq!(fixture.status(), None);
}

#[test]
fn theme_component_removal_releases_bindings_and_recovers_without_resetting_controls() {
    for component in [ComponentValue::GUI_THEME, ComponentValue::GUI_THEME_MOTION] {
        for settled in [false, true] {
            let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
            fixture.action(GuiLocalAction::Toggle);
            fixture.frame(if settled {
                1.0
            } else {
                0.25
            });
            let key = fixture.key();
            assert!(
                Arc::strong_count(
                    &fixture
                        .host
                        .asset_resources()
                        .get_typed::<AnimationClip>(key)
                        .unwrap()
                        .tracks()[0]
                ) > 1
            );
            let committed = fixture.read(fixture.control).unwrap().value;

            // Keep the unbound clip resident through the removal frame so
            // recovery reuses it instead of reloading.
            fixture
                .host
                .asset_resources_mut()
                .set_idle_resident_bytes_target(usize::MAX);
            apply(
                &mut fixture.host,
                fixture.world,
                vec![Command::RemoveComponent {
                    entity: EntityRef::Handle(fixture.theme),
                    component,
                }],
            )
            .result
            .unwrap();
            assert_eq!(fixture.status(), None);
            assert_eq!(
                Arc::strong_count(
                    &fixture
                        .host
                        .asset_resources()
                        .get_typed::<AnimationClip>(key)
                        .unwrap()
                        .tracks()[0]
                ),
                1
            );

            let (appearance, motion) = declarations(SOURCE);
            let value = if component == ComponentValue::GUI_THEME {
                ComponentValue::GuiTheme(appearance)
            } else {
                ComponentValue::GuiThemeMotion(motion)
            };
            apply(
                &mut fixture.host,
                fixture.world,
                vec![Command::insert_value(
                    EntityRef::Handle(fixture.theme),
                    value,
                )],
            )
            .result
            .unwrap();
            fixture.frame(0.0);
            assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
            assert_eq!(fixture.color(), BLUE);
            assert_eq!(committed, fixture.read(fixture.control).unwrap().value);
        }
    }
}

#[test]
fn committed_same_theme_source_edit_preserves_ready_origin_across_revocation() {
    for row_edit in [false, true] {
        for revoke in ["obsolete", "current", "retained"] {
            let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
            fixture.action(GuiLocalAction::Toggle);
            fixture.frame(0.25);
            let held = fixture.color();
            let retained = fixture.key();
            let prior = "fixture:///same-theme-prior";
            let current = "fixture:///same-theme-current";
            let mut motion = declarations(prior).1;
            motion.parts.get_mut(0).unwrap().source = Some(source(current));
            apply(
                &mut fixture.host,
                fixture.world,
                vec![Command::insert_value(
                    EntityRef::Handle(fixture.theme),
                    ComponentValue::GuiThemeMotion(motion),
                )],
            )
            .result
            .unwrap();
            fixture.frame(0.0);
            let prior_key = fixture.host.asset_resources().find(&source(prior)).unwrap();
            let current_key = fixture
                .host
                .asset_resources()
                .find(&source(current))
                .unwrap();
            let operations = if row_edit {
                vec![Command::SetField {
                    entity: EntityRef::Handle(fixture.theme),
                    component: ComponentValue::GUI_THEME_MOTION,
                    field: FieldWrite {
                        offset: 0x1000_0000 + 2 * 6 + 1,
                        value: FieldValue::Dynamic(DynamicValue::Asset(source(current))),
                    },
                }]
            } else {
                vec![Command::insert_value(
                    EntityRef::Handle(fixture.theme),
                    ComponentValue::GuiThemeMotion(declarations(current).1),
                )]
            };
            apply(&mut fixture.host, fixture.world, operations)
                .result
                .unwrap();
            fixture
                .host
                .asset_resources_mut()
                .revoke_resource(match revoke {
                    "obsolete" => prior_key,
                    "current" => current_key,
                    _ => retained,
                });
            fixture.host.flush_resource_lifecycle();
            assert_eq!(
                fixture.status(),
                Some(if revoke == "current" {
                    GuiSkinMotionStatus::Unavailable
                } else {
                    GuiSkinMotionStatus::Pending
                }),
                "row_edit={row_edit} revoke={revoke}"
            );

            fixture.frame(0.0);
            assert_eq!(
                fixture.color(),
                if revoke == "obsolete" {
                    held
                } else {
                    BLUE
                }
            );
            if revoke == "current" {
                assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
            }
            fixture
                .host
                .asset_resources_mut()
                .register_client_source(fixture.world, source(current), clip().encode())
                .unwrap();
            for _ in 0..8 {
                fixture.host.progress_evaluation_assets();
            }
            fixture.frame(0.0);
            assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
            let origin = if revoke == "obsolete" {
                held
            } else {
                BLUE
            };
            assert_eq!(fixture.color(), origin);
            fixture.frame(0.5);
            assert_eq!(
                fixture.color(),
                std::array::from_fn(|channel| { (origin[channel] + BLUE[channel]) / 2.0 })
            );
        }
    }
}

#[test]
fn committed_selector_changes_reconcile_source_watches_without_preparation() {
    for ancestor in [false, true] {
        let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
        let target = if ancestor {
            let parent = create(
                &mut fixture.host,
                fixture.world,
                vec![ComponentValue::GuiBehavior(GuiBehavior::default())],
                None,
            );
            apply(
                &mut fixture.host,
                fixture.world,
                vec![Command::PlaceEntity {
                    entity: EntityRef::Handle(fixture.control),
                    placement: EntityPlacementRef {
                        parent: Some(EntityRef::Handle(parent)),
                        before: None,
                    },
                }],
            )
            .result
            .unwrap();
            parent
        } else {
            fixture.control
        };
        fixture.action(GuiLocalAction::Toggle);
        fixture.frame(0.25);
        let prior = "fixture:///enabled-pending";
        let next = "fixture:///disabled-pending";
        let mut motion = declarations(prior).1;
        motion.parts.get_mut(1).unwrap().source = Some(source(next));
        apply(
            &mut fixture.host,
            fixture.world,
            vec![Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiThemeMotion(motion),
            )],
        )
        .result
        .unwrap();
        fixture.frame(0.0);
        let prior_key = fixture.host.asset_resources().find(&source(prior)).unwrap();
        let next_key = fixture.host.asset_resources().find(&source(next)).unwrap();
        apply(
            &mut fixture.host,
            fixture.world,
            vec![Command::SetField {
                entity: EntityRef::Handle(target),
                component: ComponentValue::GUI_BEHAVIOR,
                field: FieldWrite {
                    offset: std::mem::offset_of!(GuiBehavior, enabled) as u32,
                    value: FieldValue::Bool(false),
                },
            }],
        )
        .result
        .unwrap();
        fixture
            .host
            .asset_resources_mut()
            .revoke_resource(prior_key);
        fixture.host.flush_resource_lifecycle();
        assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Pending));
        fixture.host.asset_resources_mut().revoke_resource(next_key);
        fixture.host.flush_resource_lifecycle();
        assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
        fixture.frame(0.0);
        assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    }
}

#[test]
fn unchanged_requested_source_revocation_survives_unprepared_recipe_edits() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    let uri = "fixture:///unchanged-edited-request";
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::insert_value(
            EntityRef::Handle(fixture.theme),
            ComponentValue::GuiThemeMotion(declarations(uri).1),
        )],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    let pending = fixture.host.asset_resources().find(&source(uri)).unwrap();
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::SetField {
            entity: EntityRef::Handle(fixture.theme),
            component: ComponentValue::GUI_THEME_MOTION,
            field: FieldWrite {
                offset: 0x1000_0000 + 2 * 6 + 2,
                value: FieldValue::Dynamic(DynamicValue::F32(2.0)),
            },
        }],
    )
    .result
    .unwrap();
    fixture.host.asset_resources_mut().revoke_resource(pending);
    fixture.host.flush_resource_lifecycle();
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    fixture.frame(0.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    assert_eq!(fixture.color(), BLUE);
}

#[test]
fn committed_retarget_preserves_ready_origin_when_old_theme_is_deleted() {
    check_committed_retarget(true);
}

#[test]
fn committed_retarget_withdraws_when_new_theme_is_deleted() {
    check_committed_retarget(false);
}

fn check_committed_retarget(delete_old: bool) {
    for stage in ["active", "settled", "pending"] {
        for same_batch in [false, true] {
            let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
            fixture.action(GuiLocalAction::Toggle);
            fixture.frame(if stage == "settled" {
                1.0
            } else {
                0.25
            });
            if stage == "pending" {
                apply(
                    &mut fixture.host,
                    fixture.world,
                    vec![Command::insert_value(
                        EntityRef::Handle(fixture.theme),
                        ComponentValue::GuiThemeMotion(
                            declarations("fixture:///prior-pending-retarget").1,
                        ),
                    )],
                )
                .result
                .unwrap();
                fixture.frame(0.0);
            }
            let old_key = fixture.key();
            let old_theme = fixture.theme;
            let held = fixture.color();
            let committed = fixture.read(fixture.control).unwrap().value;
            let (appearance, motion) = declarations("fixture:///committed-retarget");
            let next = create(
                &mut fixture.host,
                fixture.world,
                vec![
                    ComponentValue::GuiTheme(appearance),
                    ComponentValue::GuiThemeMotion(motion),
                ],
                None,
            );
            let retarget = Command::SetField {
                entity: EntityRef::Handle(fixture.control),
                component: ComponentValue::GUI_SKIN,
                field: FieldWrite {
                    offset: std::mem::offset_of!(GuiSkin, theme) as u32,
                    value: FieldValue::Entity(EntityRef::Handle(next)),
                },
            };
            let mut operations = Vec::new();
            if same_batch {
                operations.push(retarget);
            } else {
                apply(&mut fixture.host, fixture.world, vec![retarget])
                    .result
                    .unwrap();
            }
            operations.push(Command::Delete {
                entity: EntityRef::Handle(if delete_old {
                    old_theme
                } else {
                    next
                }),
            });
            fixture
                .host
                .asset_resources_mut()
                .set_idle_resident_bytes_target(0);
            apply(&mut fixture.host, fixture.world, operations)
                .result
                .unwrap();

            // The applying frame evaluated the retarget, so the control now
            // waits for the new theme's motion while holding its ready origin.
            let status = delete_old.then_some(GuiSkinMotionStatus::Pending);
            assert_eq!(fixture.status(), status, "{stage} same_batch={same_batch}");
            if delete_old && stage == "pending" {
                // The evaluated retarget already withdrew the obsolete request.
                assert!(
                    fixture
                        .host
                        .asset_resources()
                        .find(&source("fixture:///prior-pending-retarget"))
                        .is_none()
                );
            }
            fixture.host.flush_resource_lifecycle();
            assert_eq!(fixture.status(), status);
            if delete_old {
                assert!(
                    fixture
                        .host
                        .asset_resources()
                        .get_typed::<AnimationClip>(old_key)
                        .is_some()
                );
            } else if stage == "pending" {
                assert!(
                    fixture
                        .host
                        .asset_resources()
                        .get_typed::<AnimationClip>(old_key)
                        .is_none()
                );
            } else {
                assert_eq!(
                    Arc::strong_count(
                        &fixture
                            .host
                            .asset_resources()
                            .get_typed::<AnimationClip>(old_key)
                            .unwrap()
                            .tracks()[0]
                    ),
                    1
                );
            }

            fixture.frame(0.0);
            if delete_old {
                assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Pending));
                assert_eq!(fixture.color(), held);
            } else {
                assert_eq!(fixture.status(), None);
            }
            fixture
                .host
                .asset_resources_mut()
                .register_client_source(
                    fixture.world,
                    source("fixture:///committed-retarget"),
                    clip().encode(),
                )
                .unwrap();
            for _ in 0..8 {
                fixture.host.progress_evaluation_assets();
            }
            fixture.frame(0.0);
            if delete_old {
                assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
                assert_eq!(fixture.color(), held);
                fixture.frame(0.5);
                assert_eq!(
                    fixture.color(),
                    std::array::from_fn(|channel| (held[channel] + BLUE[channel]) / 2.0)
                );
            } else {
                assert_eq!(fixture.status(), None);
            }
            assert_eq!(committed, fixture.read(fixture.control).unwrap().value);
        }
    }
}

#[test]
fn committed_retarget_round_trip_rearms_the_unchanged_request() {
    for stage in ["active", "settled", "pending"] {
        let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
        fixture.action(GuiLocalAction::Toggle);
        fixture.frame(if stage == "settled" {
            1.0
        } else {
            0.25
        });
        if stage == "pending" {
            apply(
                &mut fixture.host,
                fixture.world,
                vec![Command::insert_value(
                    EntityRef::Handle(fixture.theme),
                    ComponentValue::GuiThemeMotion(declarations("fixture:///round-trip-pending").1),
                )],
            )
            .result
            .unwrap();
            fixture.frame(0.0);
        }
        let (appearance, motion) = declarations("fixture:///round-trip-unused");
        let other = create(
            &mut fixture.host,
            fixture.world,
            vec![
                ComponentValue::GuiTheme(appearance),
                ComponentValue::GuiThemeMotion(motion),
            ],
            None,
        );
        for theme in [other, fixture.theme] {
            apply(
                &mut fixture.host,
                fixture.world,
                vec![Command::SetField {
                    entity: EntityRef::Handle(fixture.control),
                    component: ComponentValue::GUI_SKIN,
                    field: FieldWrite {
                        offset: std::mem::offset_of!(GuiSkin, theme) as u32,
                        value: FieldValue::Entity(EntityRef::Handle(theme)),
                    },
                }],
            )
            .result
            .unwrap();
        }
        fixture.frame(0.25);
        assert_eq!(
            fixture.color(),
            match stage {
                "active" => [0.5, 0.0, 0.5, 1.0],
                "settled" => BLUE,
                _ => [0.75, 0.0, 0.25, 1.0],
            }
        );
        if stage != "pending" {
            let (_, sampling) = fixture
                .host
                .world_mut(fixture.world)
                .unwrap()
                .gui_motion_work()
                .unwrap();
            assert_eq!(sampling.bindings, 0);
            assert_eq!(sampling.samples, usize::from(stage == "active"));
        }
        let uri = if stage == "pending" {
            "fixture:///round-trip-pending"
        } else {
            SOURCE
        };
        let key = fixture.host.asset_resources().find(&source(uri)).unwrap();
        fixture.host.asset_resources_mut().revoke_resource(key);
        fixture.host.flush_resource_lifecycle();
        assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    }
}

#[test]
fn retargeted_pending_motion_ignores_old_theme_deletion_and_reused_theme_slot() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    let old_theme = fixture.theme;
    let old_key = fixture.key();
    let (appearance, motion) = declarations("fixture:///retargeted-theme");
    let next = create(
        &mut fixture.host,
        fixture.world,
        vec![
            ComponentValue::GuiTheme(appearance),
            ComponentValue::GuiThemeMotion(motion),
        ],
        None,
    );
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::SetField {
            entity: EntityRef::Handle(fixture.control),
            component: ComponentValue::GUI_SKIN,
            field: FieldWrite {
                offset: std::mem::offset_of!(GuiSkin, theme) as u32,
                value: FieldValue::Entity(EntityRef::Handle(next)),
            },
        }],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Pending));
    let held = fixture.color();
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::Delete {
            entity: EntityRef::Handle(old_theme),
        }],
    )
    .result
    .unwrap();
    fixture.frame(0.25);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Pending));
    assert_eq!(fixture.color(), held);
    assert!(
        fixture
            .host
            .asset_resources()
            .get_typed::<AnimationClip>(old_key)
            .is_some()
    );
    fixture.load("fixture:///retargeted-theme");
    fixture.frame(0.25);
    assert_eq!(fixture.color(), [0.5625, 0.0, 0.4375, 1.0]);

    // Each apply runs a frame; keep the unbound clip resident so the
    // replacement theme's retarget can reuse it.
    fixture
        .host
        .asset_resources_mut()
        .set_idle_resident_bytes_target(usize::MAX);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::Delete {
            entity: EntityRef::Handle(next),
        }],
    )
    .result
    .unwrap();
    let (appearance, motion) = declarations("fixture:///retargeted-theme");
    let replacement = create(
        &mut fixture.host,
        fixture.world,
        vec![
            ComponentValue::GuiTheme(appearance),
            ComponentValue::GuiThemeMotion(motion),
        ],
        None,
    );
    assert_eq!(replacement.index(), next.index());
    assert_ne!(replacement, next);
    fixture.frame(0.0);
    assert_eq!(fixture.status(), None);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::SetField {
            entity: EntityRef::Handle(fixture.control),
            component: ComponentValue::GUI_SKIN,
            field: FieldWrite {
                offset: std::mem::offset_of!(GuiSkin, theme) as u32,
                value: FieldValue::Entity(EntityRef::Handle(replacement)),
            },
        }],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    assert_eq!(fixture.color(), BLUE);
}

#[test]
fn withdrawing_motion_demand_does_not_release_an_ordinary_animation_owner() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let mut material = ipp_core::components::CustomMaterial::default();
    material
        .properties
        .set("ordinary", DynamicValue::F32(0.25))
        .unwrap();
    let target = create(
        &mut fixture.host,
        fixture.world,
        vec![ComponentValue::CustomMaterial(material)],
        None,
    );
    let ordinary = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: SOURCE.into(),
                variant: 0,
                track: 1,
                target,
                property: AnimationTrackTarget::DynamicProperty {
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: "ordinary".into(),
                },
                entity_bindings: Vec::new(),
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            }],
            speed: 0.0,
            looping: false,
        })
        .unwrap();
    fixture.frame(0.0);
    let key = fixture.key();
    let before = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .animation_controller(ordinary)
        .unwrap();
    fixture
        .host
        .asset_resources_mut()
        .set_idle_resident_bytes_target(0);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::Delete {
            entity: EntityRef::Handle(fixture.theme),
        }],
    )
    .result
    .unwrap();
    fixture.frame(0.25);
    fixture.host.flush_resource_lifecycle();
    assert_eq!(fixture.status(), None);
    assert!(
        fixture
            .host
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .is_some()
    );
    assert_eq!(
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .animation_controller(ordinary)
            .unwrap(),
        before
    );
    fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .remove_animation_controller(ordinary)
        .unwrap();
    fixture.frame(0.0);
    fixture.host.flush_resource_lifecycle();
    assert!(
        fixture
            .host
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .is_none()
    );
}

#[test]
fn pending_replacement_holds_last_ready_and_its_demand_then_revocation_withdraws() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    let held = fixture.color();
    let old = fixture.key();
    let (_, motion) = declarations("fixture:///replacement-motion");
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::insert_value(
            EntityRef::Handle(fixture.theme),
            ComponentValue::GuiThemeMotion(motion),
        )],
    )
    .result
    .unwrap();
    fixture.frame(0.25);
    assert_eq!(fixture.color(), held);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Pending));
    fixture.host.flush_resource_lifecycle();
    assert!(
        fixture
            .host
            .asset_resources()
            .get_typed::<AnimationClip>(old)
            .is_some()
    );
    fixture.load("fixture:///replacement-motion");
    assert_eq!(fixture.color(), held);
    fixture.frame(0.5);
    assert_eq!(fixture.color(), [0.375, 0.0, 0.625, 1.0]);
    let replacement = fixture
        .host
        .asset_resources()
        .find(&source("fixture:///replacement-motion"))
        .unwrap();
    fixture
        .host
        .asset_resources_mut()
        .revoke_resource(replacement);
    fixture.host.flush_resource_lifecycle();
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    fixture.frame(0.0);
    assert_eq!(fixture.color(), BLUE);
}

#[test]
fn unloading_releases_prepared_tracks_and_suspends_elapsed_until_same_source_ready() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    let key = fixture.key();
    let old_track = Arc::downgrade(
        &fixture
            .host
            .asset_resources()
            .get_typed::<AnimationClip>(key)
            .unwrap()
            .tracks()[0],
    );
    fixture.host.asset_resources_mut().unload(key);
    fixture.host.flush_resource_lifecycle();
    assert!(old_track.upgrade().is_none());
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Pending));
    fixture.load(SOURCE);
    assert_eq!(fixture.color(), [0.75, 0.0, 0.25, 1.0]);
    fixture.frame(0.25);
    assert_eq!(fixture.color(), [0.5, 0.0, 0.5, 1.0]);
}

#[test]
fn pending_replacement_ready_before_retired_source_revocation_remains_retryable() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    let old = fixture.key();
    let replacement = "fixture:///ready-before-revoke";
    let (_, motion) = declarations(replacement);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::insert_value(
            EntityRef::Handle(fixture.theme),
            ComponentValue::GuiThemeMotion(motion),
        )],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    for _ in 0..8 {
        fixture.host.progress_evaluation_assets();
    }
    let requests = fixture.host.take_resource_requests();
    assert_eq!(requests.len(), 1);
    fixture
        .host
        .complete_resource(requests[0].id, Ok(clip().encode()))
        .unwrap();
    for _ in 0..8 {
        fixture.host.progress_evaluation_assets();
    }
    let ready = fixture
        .host
        .asset_resources()
        .find(&source(replacement))
        .unwrap();
    assert!(
        fixture
            .host
            .asset_resources()
            .get_typed::<AnimationClip>(ready)
            .is_some()
    );
    fixture.host.asset_resources_mut().revoke_resource(old);
    fixture.host.flush_resource_lifecycle();
    fixture.frame(0.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    assert_eq!(fixture.color(), BLUE);
}

fn focus_opacity(fixture: &Fixture) -> f32 {
    fixture
        .publication()
        .entries
        .iter()
        .find_map(|entry| match entry.as_ref() {
            CanvasPaintEntry::Primitive {
                primitive,
                ..
            } if primitive.style().identity.target.entity == fixture.control
                && primitive.style().identity.part
                    == ipp_core::systems::canvas::CanvasPart::FocusRing =>
            {
                Some(primitive.style().opacity)
            }
            _ => None,
        })
        .unwrap_or(0.0)
}

#[test]
fn focus_and_blur_use_separate_continuous_motion_with_interruption() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let (mut appearance, mut motion) = declarations(SOURCE);
    let key = GuiPartId::base(GuiPrimitivePart::FocusRing)
        .index()
        .unwrap();
    appearance
        .parts
        .insert(
            key,
            GuiPaintPart {
                part: key,
                color: Some(RED),
                opacity: Some(1.0),
                scale: Some([1.0; 2]),
                ..Default::default()
            },
        )
        .unwrap();
    motion
        .parts
        .insert(
            key,
            GuiMotionPart {
                part: key,
                source: Some(source(SOURCE)),
                duration: Some(1.0),
                easing: Some(0),
                track: Some(0),
                time: Some(0.0),
            },
        )
        .unwrap();
    apply(
        &mut fixture.host,
        fixture.world,
        vec![
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiTheme(appearance),
            ),
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiThemeMotion(motion),
            ),
        ],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    assert_eq!(focus_opacity(&fixture), 0.0);
    let committed = fixture.read(fixture.control).unwrap().value;
    let layout = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .gui_entity_layout_statistics()
        .unwrap()
        .total;
    fixture.action(GuiLocalAction::Focus);
    fixture.frame(0.25);
    assert_eq!(focus_opacity(&fixture), 0.25);
    fixture.action(GuiLocalAction::Blur);
    fixture.frame(0.5);
    assert_eq!(focus_opacity(&fixture), 0.125);
    assert!(!fixture.read(fixture.control).unwrap().focused);
    fixture.action(GuiLocalAction::Focus);
    fixture.frame(0.5);
    assert_eq!(focus_opacity(&fixture), 0.5625);
    fixture.frame(0.5);
    assert_eq!(focus_opacity(&fixture), 1.0);
    fixture.action(GuiLocalAction::Blur);
    fixture.frame(1.0);
    assert_eq!(focus_opacity(&fixture), 0.0);
    let snapshot = fixture.read(fixture.control).unwrap();
    assert!(committed == snapshot.value);
    assert_eq!(
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .gui_entity_layout_statistics()
            .unwrap()
            .total,
        layout
    );
}

fn install_focus_motion(fixture: &mut Fixture, uri: &str) {
    let (mut appearance, mut motion) = declarations(SOURCE);
    let part = GuiPartId::base(GuiPrimitivePart::FocusRing)
        .index()
        .unwrap();
    appearance
        .parts
        .insert(
            part,
            GuiPaintPart {
                part,
                color: Some(RED),
                opacity: Some(1.0),
                scale: Some([1.0; 2]),
                ..Default::default()
            },
        )
        .unwrap();
    motion
        .parts
        .insert(
            part,
            GuiMotionPart {
                part,
                source: Some(source(uri)),
                duration: Some(1.0),
                easing: Some(0),
                track: Some(0),
                time: Some(0.0),
            },
        )
        .unwrap();
    apply(
        &mut fixture.host,
        fixture.world,
        vec![
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiTheme(appearance),
            ),
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiThemeMotion(motion),
            ),
        ],
    )
    .result
    .unwrap();
}

fn check_initially_focused_blur(delayed: bool) {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    fixture.action(GuiLocalAction::Focus);
    fixture.frame(0.0);
    let uri = if delayed {
        "fixture:///initially-focused"
    } else {
        SOURCE
    };
    install_focus_motion(&mut fixture, uri);
    fixture.frame(0.0);
    if delayed {
        fixture.load(uri);
    }
    assert_eq!(focus_opacity(&fixture), 1.0);
    let committed = fixture.read(fixture.control).unwrap().value;
    fixture.action(GuiLocalAction::Blur);
    fixture.frame(0.5);
    assert_eq!(focus_opacity(&fixture), 0.5);
    if delayed {
        let key = fixture.host.asset_resources().find(&source(uri)).unwrap();
        fixture.host.asset_resources_mut().unload(key);
        fixture.host.flush_resource_lifecycle();
        fixture.load(uri);
        assert_eq!(focus_opacity(&fixture), 0.5);
    }
    fixture.action(GuiLocalAction::Focus);
    fixture.frame(0.5);
    assert_eq!(focus_opacity(&fixture), 0.75);
    fixture.action(GuiLocalAction::Blur);
    fixture.frame(0.5);
    assert_eq!(focus_opacity(&fixture), 0.375);
    fixture.frame(0.5);
    assert_eq!(focus_opacity(&fixture), 0.0);
    let snapshot = fixture.read(fixture.control).unwrap();
    assert!(!snapshot.focused);
    assert!(committed == snapshot.value);
}

#[test]
fn initially_focused_first_binding_blurs_to_zero_and_preserves_interruption() {
    check_initially_focused_blur(false);
}

#[test]
fn initially_focused_delayed_ready_blurs_to_zero_and_preserves_interruption() {
    check_initially_focused_blur(true);
}

fn reuse_control_slot(fixture: &mut Fixture, theme: EntityId) -> EntityId {
    let old = fixture.control;
    let parent = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .entity_link(old)
        .unwrap()
        .parent;
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::Delete {
            entity: EntityRef::Handle(old),
        }],
    )
    .result
    .unwrap();
    fixture.control = create(
        &mut fixture.host,
        fixture.world,
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiBehavior(GuiBehavior::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 40.0,
                height: 20.0,
                ..Default::default()
            }),
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                ..Default::default()
            }),
        ],
        parent,
    );
    assert_eq!(old.index(), fixture.control.index());
    assert_ne!(old, fixture.control);
    old
}

#[test]
fn retired_entity_dirty_theme_cannot_clear_reused_slot_motion() {
    check_reused_slot_theme(true);
}

#[test]
fn retired_entity_dirty_theme_cannot_clear_retained_skin_incarnation() {
    check_reused_slot_theme(false);
}

#[test]
fn corrected_control_before_preparation_rebinds_the_retained_incarnation() {
    let mut fixture = Fixture::new(ComponentValue::GuiSlider(GuiSlider::default()));
    let before = fixture.read(fixture.control).unwrap();
    fixture.enabled(false);
    fixture.frame(0.25);
    assert_eq!(fixture.color(), [0.75, 0.0, 0.25, 1.0]);
    for (minimum, valid) in [(2.0, false), (0.0, true)] {
        let result = apply(
            &mut fixture.host,
            fixture.world,
            vec![Command::SetField {
                entity: EntityRef::Handle(fixture.control),
                component: ComponentValue::GUI_SLIDER,
                field: FieldWrite {
                    offset: std::mem::offset_of!(GuiSlider, min) as u32,
                    value: FieldValue::F32(minimum),
                },
            }],
        );
        assert_eq!(result.result.is_ok(), valid);
    }
    fixture.frame(1.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    assert_eq!(fixture.color(), BLUE);
    let after = fixture.read(fixture.control).unwrap();
    assert_eq!(before.target, after.target);
    assert!(before.value == after.value);
}

fn check_reused_slot_theme(replace_skin: bool) {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let old_theme = fixture.theme;
    let retired = reuse_control_slot(&mut fixture, old_theme);
    fixture.frame(0.0);
    let (appearance, motion) = declarations(SOURCE);
    let new_theme = create(
        &mut fixture.host,
        fixture.world,
        vec![
            ComponentValue::GuiTheme(appearance),
            ComponentValue::GuiThemeMotion(motion),
        ],
        None,
    );
    let command = if replace_skin {
        Command::insert_value(
            EntityRef::Handle(fixture.control),
            ComponentValue::GuiSkin(GuiSkin {
                theme: new_theme,
                ..Default::default()
            }),
        )
    } else {
        Command::SetField {
            entity: EntityRef::Handle(fixture.control),
            component: ComponentValue::GUI_SKIN,
            field: FieldWrite {
                offset: std::mem::offset_of!(GuiSkin, theme) as u32,
                value: FieldValue::Entity(EntityRef::Handle(new_theme)),
            },
        }
    };
    apply(&mut fixture.host, fixture.world, vec![command])
        .result
        .unwrap();
    fixture.frame(0.0);
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    assert_eq!(fixture.color(), [0.75, 0.0, 0.25, 1.0]);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::insert_value(
            EntityRef::Handle(old_theme),
            ComponentValue::GuiTheme(declarations(SOURCE).0),
        )],
    )
    .result
    .unwrap();
    fixture.frame(0.25);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    assert_eq!(fixture.color(), [0.5, 0.0, 0.5, 1.0]);
    assert!(fixture.read(retired).is_none());
}

#[test]
fn sparse_ancestry_rebinds_after_reparent_cycle_and_correction() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let canvas = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .entity_link(fixture.control)
        .unwrap()
        .parent;
    let parent = create(
        &mut fixture.host,
        fixture.world,
        vec![ComponentValue::GuiBehavior(GuiBehavior {
            enabled: false,
            ..Default::default()
        })],
        canvas,
    );
    let place = |entity, parent: Option<EntityId>| Command::PlaceEntity {
        entity: EntityRef::Handle(entity),
        placement: EntityPlacementRef {
            parent: parent.map(EntityRef::Handle),
            before: None,
        },
    };
    apply(
        &mut fixture.host,
        fixture.world,
        vec![place(fixture.control, Some(parent))],
    )
    .result
    .unwrap();
    fixture.frame(0.5);
    assert_eq!(fixture.color(), [0.5, 0.0, 0.5, 1.0]);
    assert!(
        apply(
            &mut fixture.host,
            fixture.world,
            vec![place(parent, Some(fixture.control))]
        )
        .result
        .is_err()
    );
    fixture.frame(0.0);
    assert_eq!(fixture.status(), None);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![place(parent, canvas)],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    assert_eq!(fixture.color(), BLUE);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![place(fixture.control, canvas)],
    )
    .result
    .unwrap();
    fixture.frame(0.5);
    assert_eq!(fixture.color(), [0.5, 0.0, 0.5, 1.0]);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::insert_value(
            EntityRef::Handle(parent),
            ComponentValue::GuiBehavior(GuiBehavior::default()),
        )],
    )
    .result
    .unwrap();
    fixture.frame(0.5);
    assert_eq!(fixture.color(), RED);
    assert_eq!(
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .gui_motion_work()
            .unwrap()
            .0,
        Default::default()
    );
}

#[test]
fn motion_work_is_sparse_in_large_static_and_settled_worlds() {
    for animated in [false, true] {
        let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
        if !animated {
            apply(
                &mut fixture.host,
                fixture.world,
                vec![Command::RemoveComponent {
                    entity: EntityRef::Handle(fixture.theme),
                    component: ComponentValue::GUI_THEME_MOTION,
                }],
            )
            .result
            .unwrap();
        }
        let static_theme = create(
            &mut fixture.host,
            fixture.world,
            vec![ComponentValue::GuiTheme(declarations(SOURCE).0)],
            None,
        );
        let mut operations = Vec::new();
        for alias in 1..=1024 {
            operations.push(Command::Create {
                alias,
                metadata: Default::default(),
                adopt: false,
            });
            operations.push(Command::insert_value(
                EntityRef::Alias(alias),
                ComponentValue::GuiButton(GuiButton::default()),
            ));
            operations.push(Command::insert_value(
                EntityRef::Alias(alias),
                ComponentValue::GuiSkin(GuiSkin {
                    theme: if alias <= 512 {
                        static_theme
                    } else {
                        fixture.theme
                    },
                    ..Default::default()
                }),
            ));
        }
        let created = apply(&mut fixture.host, fixture.world, operations)
            .result
            .unwrap();
        let static_control = created[0].1;
        let motion_control = created[512].1;
        let camera = create(
            &mut fixture.host,
            fixture.world,
            vec![ComponentValue::Camera(Default::default())],
            None,
        );
        fixture.frame(0.0);
        fixture.frame(0.0);
        let work = |fixture: &mut Fixture| {
            fixture
                .host
                .world_mut(fixture.world)
                .unwrap()
                .gui_motion_work()
                .unwrap()
        };
        assert_eq!(work(&mut fixture), Default::default());
        // Every control is canvas content: restyling one repaints it, while an
        // identical skin write and a camera edit leave the paint untouched.
        for (operation, repaints) in [
            (
                Command::insert_value(
                    EntityRef::Handle(static_control),
                    ComponentValue::GuiSkin(GuiSkin {
                        theme: static_theme,
                        ..Default::default()
                    }),
                ),
                false,
            ),
            (
                Command::insert_value(
                    EntityRef::Handle(static_control),
                    ComponentValue::CanvasStyle(ipp_core::systems::canvas::CanvasStyle {
                        opacity: 0.5,
                        ..Default::default()
                    }),
                ),
                true,
            ),
            (
                Command::SetField {
                    entity: EntityRef::Handle(camera),
                    component: ComponentValue::CAMERA,
                    field: FieldWrite {
                        offset: std::mem::offset_of!(ipp_core::components::Camera, fov_y) as u32,
                        value: FieldValue::F32(1.0),
                    },
                },
                false,
            ),
        ] {
            let before = fixture.publication();
            apply(&mut fixture.host, fixture.world, vec![operation])
                .result
                .unwrap();
            fixture.frame(0.1);
            assert_eq!(work(&mut fixture), Default::default());
            assert_eq!(
                !Arc::ptr_eq(&before.entries, &fixture.publication().entries),
                repaints
            );
        }

        apply(
            &mut fixture.host,
            fixture.world,
            vec![Command::insert_value(
                EntityRef::Handle(motion_control),
                ComponentValue::CanvasStyle(ipp_core::systems::canvas::CanvasStyle {
                    opacity: 0.5,
                    ..Default::default()
                }),
            )],
        )
        .result
        .unwrap();

        // The applying frame prepared the edited control.
        let (preparation, sampling) = work(&mut fixture);
        assert_eq!(preparation.snapshots, usize::from(animated));
        assert_eq!(preparation.parts, 5 * usize::from(animated));
        assert_eq!(sampling, Default::default());

        fixture.action(GuiLocalAction::Toggle);
        fixture.frame(0.25);
        let (preparation, sampling) = work(&mut fixture);
        assert_eq!(preparation.snapshots, usize::from(animated));
        assert_eq!(preparation.parts, 5 * usize::from(animated));
        assert_eq!(sampling.owners, usize::from(animated));
        assert_eq!(sampling.bindings, usize::from(animated));
        assert_eq!(sampling.samples, usize::from(animated));
        fixture.frame(0.25);
        let (preparation, sampling) = work(&mut fixture);
        assert_eq!(preparation, Default::default());
        assert_eq!(sampling.owners, usize::from(animated));
        assert_eq!(sampling.bindings, 0);
        assert_eq!(sampling.samples, usize::from(animated));
        fixture.frame(0.5);
        fixture.frame(0.5);
        assert_eq!(work(&mut fixture), Default::default());
    }
}

#[test]
fn retiring_source_release_order_and_kind_do_not_poison_requested_identity() {
    for ready_first in [false, true] {
        for revoke in [false, true] {
            let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
            fixture.action(GuiLocalAction::Toggle);
            fixture.frame(0.25);
            let old = fixture.key();
            let uri = "fixture:///retiring-order";
            apply(
                &mut fixture.host,
                fixture.world,
                vec![Command::insert_value(
                    EntityRef::Handle(fixture.theme),
                    ComponentValue::GuiThemeMotion(declarations(uri).1),
                )],
            )
            .result
            .unwrap();
            fixture.frame(0.0);
            for _ in 0..8 {
                fixture.host.progress_evaluation_assets();
            }
            let requests = fixture.host.take_resource_requests();
            assert_eq!(requests.len(), 1);
            if ready_first {
                fixture
                    .host
                    .complete_resource(requests[0].id, Ok(clip().encode()))
                    .unwrap();
                for _ in 0..8 {
                    fixture.host.progress_evaluation_assets();
                }
            }
            if revoke {
                fixture.host.asset_resources_mut().revoke_resource(old);
            } else {
                fixture.host.asset_resources_mut().unload(old);
            }
            fixture.host.flush_resource_lifecycle();
            assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Pending));
            if !ready_first {
                fixture.frame(0.0);
                fixture
                    .host
                    .complete_resource(requests[0].id, Ok(clip().encode()))
                    .unwrap();
                for _ in 0..8 {
                    fixture.host.progress_evaluation_assets();
                }
            }
            fixture.frame(0.0);
            assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
            assert_eq!(fixture.color(), BLUE);
        }
    }
}

#[test]
fn revoked_pending_destination_does_not_keep_or_revive_retired_appearance() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    let old = fixture.key();
    let uri = "fixture:///revoked-pending";
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::insert_value(
            EntityRef::Handle(fixture.theme),
            ComponentValue::GuiThemeMotion(declarations(uri).1),
        )],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    let pending = fixture.host.asset_resources().find(&source(uri)).unwrap();
    fixture.host.asset_resources_mut().revoke_resource(pending);
    fixture.host.flush_resource_lifecycle();
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    fixture.host.asset_resources_mut().revoke_resource(old);
    fixture.host.flush_resource_lifecycle();
    fixture.frame(0.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    assert_eq!(fixture.color(), BLUE);
    fixture
        .host
        .asset_resources_mut()
        .register_client_source(fixture.world, source(uri), clip().encode())
        .unwrap();
    for _ in 0..8 {
        fixture.host.progress_evaluation_assets();
    }
    let replacement = fixture.host.asset_resources().find(&source(uri)).unwrap();
    assert_ne!(replacement, pending);
    fixture.frame(0.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    assert_eq!(fixture.color(), BLUE);
}

#[test]
fn static_gui_does_not_select_animation_but_motion_requires_animation_and_assets() {
    use ipp_core::systems::asset_dependencies::AssetDependencySystem;
    for mut systems in [
        vec![GuiSystem::ID],
        vec![GuiSystem::ID, AnimationSystem::ID],
        vec![
            GuiSystem::ID,
            AnimationSystem::ID,
            AssetDependencySystem::ID,
        ],
    ] {
        let mut host = HostRuntime::new();
        let supported = systems.len() == 3;
        // Controls require CanvasBounds, so every GUI World selects Canvas.
        systems.push(ipp_core::systems::canvas::CanvasSystem::ID);
        let world = host.create_world(Default::default(), &systems).unwrap();
        let entity = create(
            &mut host,
            world,
            vec![ComponentValue::GuiButton(GuiButton::default())],
            None,
        );
        let result = apply(
            &mut host,
            world,
            vec![Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiThemeMotion(GuiThemeMotion::default()),
            )],
        );
        assert_eq!(result.result.is_ok(), supported);
    }
}

#[test]
fn generic_field_and_controller_access_cannot_claim_internal_motion_channels() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let runtime_offset = std::mem::offset_of!(GuiSkin, runtime) as u32;
    for offset in [runtime_offset, 0xffff_0000] {
        let result = apply(
            &mut fixture.host,
            fixture.world,
            vec![Command::SetField {
                entity: EntityRef::Handle(fixture.control),
                component: ComponentValue::GUI_SKIN,
                field: FieldWrite {
                    offset,
                    value: FieldValue::Dynamic(DynamicValue::Vec4(BLUE)),
                },
            }],
        );
        assert!(result.result.is_err());
        let result = fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .create_animation_controller(AnimationControllerDescription {
                drivers: vec![AnimationDriverDescription {
                    source: SOURCE.into(),
                    variant: 0,
                    track: 0,
                    target: fixture.control,
                    property: AnimationTrackTarget::AnimationProperty(AnimationProperty {
                        component: ComponentValue::GUI_SKIN,
                        offsets: vec![offset],
                    }),
                    entity_bindings: Vec::new(),
                    weight: 1.0,
                    additive: false,
                    reference_time: 0.0,
                    repeat: false,
                }],
                speed: 0.0,
                looping: false,
            });
        assert!(result.is_err());
    }
    fixture.frame(0.0);
    assert_eq!(fixture.color(), RED);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
}

#[test]
fn control_and_skin_replacement_drop_bindings_before_reuse_and_never_inherit_old_composite() {
    for replace_skin in [false, true] {
        let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
        fixture.action(GuiLocalAction::Toggle);
        fixture.frame(0.25);
        let key = fixture.key();
        assert_eq!(
            Arc::strong_count(
                &fixture
                    .host
                    .asset_resources()
                    .get_typed::<AnimationClip>(key)
                    .unwrap()
                    .tracks()[0]
            ),
            2
        );
        let replacement = if replace_skin {
            ComponentValue::GuiSkin(GuiSkin {
                theme: fixture.theme,
                ..Default::default()
            })
        } else {
            ComponentValue::GuiCheckbox(GuiCheckbox::default())
        };
        apply(
            &mut fixture.host,
            fixture.world,
            vec![Command::insert_value(
                EntityRef::Handle(fixture.control),
                replacement,
            )],
        )
        .result
        .unwrap();

        // The applying frame rebinds the replacement; the prior binding must
        // be gone rather than retained beside the new one.
        assert_eq!(
            Arc::strong_count(
                &fixture
                    .host
                    .asset_resources()
                    .get_typed::<AnimationClip>(key)
                    .unwrap()
                    .tracks()[0]
            ),
            2
        );
        fixture.frame(0.25);
        assert_eq!(
            fixture.color(),
            if replace_skin {
                BLUE
            } else {
                RED
            }
        );
    }
}

#[test]
fn malformed_ready_replacement_is_unavailable_and_correction_recovers_exact_control() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Focus);
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    let before = fixture.read(fixture.control).unwrap();
    let (appearance, mut motion) = declarations(SOURCE);
    for (_, row) in motion.parts.iter_mut() {
        row.track = Some(u32::MAX - 3);
    }
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::insert_value(
            EntityRef::Handle(fixture.theme),
            ComponentValue::GuiThemeMotion(motion),
        )],
    )
    .result
    .unwrap();
    fixture.frame(0.25);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    assert_eq!(fixture.color(), BLUE);
    let (_, motion) = declarations(SOURCE);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiTheme(appearance),
            ),
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiThemeMotion(motion),
            ),
        ],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    let after = fixture.read(fixture.control).unwrap();
    assert_eq!(before.target, after.target);
    assert!(after.focused && before.value == after.value);
}

#[test]
fn graph_restore_excludes_motion_and_retains_authored_rows_committed_values_and_ordinary_animation()
{
    use ipp_core::components::CustomMaterial;
    use ipp_core::services::world_serialization::WorldLoadOptions;
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let mut material = CustomMaterial::default();
    material
        .properties
        .set("ordinary", DynamicValue::F32(0.25))
        .unwrap();
    let target = create(
        &mut fixture.host,
        fixture.world,
        vec![ComponentValue::CustomMaterial(material)],
        None,
    );
    let ordinary = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .create_animation_controller(AnimationControllerDescription {
            drivers: vec![AnimationDriverDescription {
                source: SOURCE.into(),
                variant: 0,
                track: 1,
                target,
                property: AnimationTrackTarget::DynamicProperty {
                    component: ComponentValue::CUSTOM_MATERIAL,
                    name: "ordinary".into(),
                },
                entity_bindings: Vec::new(),
                weight: 1.0,
                additive: false,
                reference_time: 0.0,
                repeat: false,
            }],
            speed: 0.0,
            looping: false,
        })
        .unwrap();
    fixture.action(GuiLocalAction::Focus);
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    assert_eq!(
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .animation_controllers()
            .len(),
        1
    );
    assert!(
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .animation_controller(ordinary)
            .is_some()
    );
    let saved = fixture
        .host
        .save_world(fixture.world, 72, Default::default())
        .unwrap();
    let restored = fixture
        .host
        .load_world(
            &saved,
            72,
            WorldLoadOptions {
                symbolic_id: Some("motion-copy".into()),
                ..Default::default()
            },
            Default::default(),
            Default::default(),
        )
        .unwrap();
    let restored_world = restored.root.id();
    let world = fixture.host.world_mut(restored_world).unwrap();
    assert_eq!(world.animation_controllers().len(), 1);
    let mut skinned = Vec::new();
    for entity in world.entities() {
        for value in &entity.components {
            if let ComponentValue::GuiSkin(skin) = value {
                assert_eq!(skin.runtime.status(GuiPrimitivePart::Background), None);
                skinned.push(entity.id);
            }
        }
    }
    assert_eq!(skinned.len(), 1);
    drop(world);
    let control = read_control(&mut fixture.host, restored_world, skinned[0]).unwrap();
    assert_eq!(control.value, ControlValue::Bool(true));
    assert!(!control.focused);
}

#[test]
fn motion_withdrawal_restores_current_authored_appearance_without_reflow_or_stale_paint() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    assert_eq!(fixture.color(), [0.75, 0.0, 0.25, 1.0]);
    let before = fixture.publication();
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::RemoveComponent {
            entity: EntityRef::Handle(fixture.theme),
            component: ComponentValue::GUI_THEME_MOTION,
        }],
    )
    .result
    .unwrap();

    // The applying frame evaluated the withdrawal.
    assert_eq!(fixture.status(), None);
    assert_eq!(fixture.color(), BLUE);
    let after = fixture.publication();
    assert_ne!(before.paint_revision, after.paint_revision);
    assert_eq!(before.layout_revision, after.layout_revision);
}

#[test]
fn own_skin_override_precedence_and_sparse_motion_properties_match_the_clip_endpoint() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let mut appearance = Rows::default();
    appearance
        .push(GuiPaintPart {
            color: Some(BLUE),
            ..GuiPaintPart::keyed(GuiPartId::base(GuiPrimitivePart::Background)).unwrap()
        })
        .unwrap();
    let (_, mut motion) = declarations(SOURCE);
    for (_, row) in motion.parts.iter_mut() {
        row.time = Some(1.0);
        if row.part
            != GuiPartId::base(GuiPrimitivePart::Background)
                .index()
                .unwrap()
        {
            row.source = None;
            row.duration = None;
            row.easing = None;
            row.track = None;
        }
    }
    apply(
        &mut fixture.host,
        fixture.world,
        vec![
            Command::insert_value(
                EntityRef::Handle(fixture.control),
                ComponentValue::GuiSkin(GuiSkin {
                    theme: fixture.theme,
                    parts: appearance,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiThemeMotion(motion),
            ),
        ],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    assert_eq!(fixture.color(), BLUE);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    assert_eq!(fixture.color(), BLUE);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
}

#[test]
fn checkbox_indicator_fades_and_aligns_in_both_directions_without_explicit_unchecked_paint() {
    use ipp_core::systems::canvas::CanvasPart;
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let mut appearance = Rows::default();
    let mut motion = Rows::default();
    for (identity, align, time) in [
        (GuiPartId::base(GuiPrimitivePart::Icon), -1.0, 0.0),
        (
            GuiPartId::variant(
                GuiPrimitivePart::Icon,
                GuiSkinState::Idle,
                GuiPartVariant::Checked,
            ),
            1.0,
            1.0,
        ),
    ] {
        appearance
            .push(GuiPaintPart {
                color: Some(RED),
                opacity: Some(1.0),
                scale: Some([1.0; 2]),
                align_x: Some(align),
                ..GuiPaintPart::keyed(identity).unwrap()
            })
            .unwrap();
        motion
            .push(GuiMotionPart {
                part: identity.index().unwrap(),
                source: Some(source("fixture:///indicator")),
                duration: Some(1.0),
                easing: Some(0),
                track: Some(0),
                time: Some(time),
            })
            .unwrap();
    }
    apply(
        &mut fixture.host,
        fixture.world,
        vec![
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiTheme(GuiTheme {
                    parts: appearance,
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiThemeMotion(GuiThemeMotion {
                    parts: motion,
                }),
            ),
        ],
    )
    .result
    .unwrap();
    let tracks = [
        (DynamicValue::Vec4(RED), DynamicValue::Vec4(RED)),
        (DynamicValue::F32(0.0), DynamicValue::F32(1.0)),
        (DynamicValue::Vec2([1.0; 2]), DynamicValue::Vec2([1.0; 2])),
        (DynamicValue::F32(-1.0), DynamicValue::F32(1.0)),
    ]
    .into_iter()
    .enumerate()
    .map(|(channel, (first, last))| AnimationTrack {
        target: AnimationTrackTarget::DynamicProperty {
            component: ComponentValue::CUSTOM_MATERIAL,
            name: format!("lane{channel}"),
        },
        keys: vec![
            AnimationKeyframe {
                time: 0.0,
                value: AnimationValue::Field(components::schema::FieldValue::Dynamic(first)),
                interpolation: AnimationInterpolation::Linear,
            },
            AnimationKeyframe {
                time: 1.0,
                value: AnimationValue::Field(components::schema::FieldValue::Dynamic(last)),
                interpolation: AnimationInterpolation::Step,
            },
        ],
    })
    .collect();
    fixture.load_clip(
        "fixture:///indicator",
        &AnimationClip::new(1.0, tracks).unwrap(),
    );
    let icon = |fixture: &Fixture| {
        fixture
            .publication()
            .entries
            .iter()
            .find_map(|entry| match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    primitive,
                    ..
                } if primitive.style().identity.part == CanvasPart::Icon => {
                    Some(*primitive.style())
                }
                _ => None,
            })
    };
    assert!(icon(&fixture).is_none());
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.5);
    let sampled = icon(&fixture).unwrap();
    assert_eq!(sampled.opacity, 0.5);
    assert_eq!(sampled.position, [15.0, 5.0]);
    fixture.frame(0.5);
    assert_eq!(icon(&fixture).unwrap().position[0], 25.0);
    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.5);
    assert_eq!(icon(&fixture).unwrap().opacity, 0.5);
    fixture.frame(0.5);
    assert!(icon(&fixture).is_none());
}

#[test]
fn pending_motion_retains_sparse_last_ready_material_not_new_theme_values() {
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let (mut appearance, motion) = declarations("fixture:///new-material-motion");
    for (_, row) in appearance.parts.iter_mut() {
        row.corner_radius = Some([9.0; 2]);
    }
    let radius = |fixture: &Fixture| {
        fixture
            .publication()
            .entries
            .iter()
            .find_map(|entry| match entry.as_ref() {
                CanvasPaintEntry::Primitive {
                    primitive:
                        CanvasPrimitive::Box {
                            corner_radius,
                            ..
                        },
                    ..
                } => Some(*corner_radius),
                _ => None,
            })
            .unwrap()
    };
    assert_eq!(radius(&fixture), [0.0; 2]);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiTheme(appearance),
            ),
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiThemeMotion(motion),
            ),
        ],
    )
    .result
    .unwrap();
    fixture.frame(0.25);
    assert_eq!(radius(&fixture), [0.0; 2]);
    fixture.load("fixture:///new-material-motion");
    assert_eq!(radius(&fixture), [9.0; 2]);
}

/// Clip colour at time 0.5, halfway along the fixture clip's red-to-blue track.
const PURPLE: [f32; 4] = [0.5, 0.0, 0.5, 1.0];

/// Clip colour at time 0.75.
const VIOLET: [f32; 4] = [0.25, 0.0, 0.75, 1.0];

const GREEN: [f32; 4] = [0.0, 1.0, 0.0, 1.0];

fn assert_color_near(actual: [f32; 4], expected: [f32; 4]) {
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() < 1.0e-5),
        "{actual:?} != {expected:?}"
    );
}

/// Linear interpolation of a transition that started at `from`.
fn lerp(from: [f32; 4], to: [f32; 4], t: f32) -> [f32; 4] {
    std::array::from_fn(|index| from[index] + (to[index] - from[index]) * t)
}

impl Fixture {
    /// Replace the theme with the fixture declarations plus extra state rows,
    /// each sampling the shared clip at its own time.
    fn with_state_rows(&mut self, rows: &[(GuiPartId, [f32; 4], f32)]) {
        let (mut appearance, mut motion) = declarations(SOURCE);
        for (part, color, time) in rows {
            appearance
                .parts
                .push(GuiPaintPart {
                    color: Some(*color),
                    opacity: Some(1.0),
                    scale: Some([1.0; 2]),
                    ..GuiPaintPart::keyed(*part).unwrap()
                })
                .unwrap();
            motion
                .parts
                .push(GuiMotionPart {
                    part: part.index().unwrap(),
                    source: Some(source(SOURCE)),
                    duration: Some(1.0),
                    easing: Some(0),
                    track: Some(0),
                    time: Some(*time),
                })
                .unwrap();
        }
        apply(
            &mut self.host,
            self.world,
            vec![
                Command::insert_value(
                    EntityRef::Handle(self.theme),
                    ComponentValue::GuiTheme(appearance),
                ),
                Command::insert_value(
                    EntityRef::Handle(self.theme),
                    ComponentValue::GuiThemeMotion(motion),
                ),
            ],
        )
        .result
        .unwrap();
        self.frame(0.0);
    }

    /// Present the Canvas as a root so physical pointer feedback can be reserved.
    fn pointer_context(&mut self) -> ipp_core::services::gui_input::GuiInputContext {
        self.host
            .set_root_output(
                self.output,
                WorldViewport {
                    width: 100,
                    height: 100,
                    device_pixel_ratio: 1.0,
                },
            )
            .unwrap();
        self.frame(0.0);
        self.input
            .bind_context(&self.host, &self.session, self.output.world())
            .unwrap()
            .context
    }

    fn pointer(
        &mut self,
        context: &ipp_core::services::gui_input::GuiInputContext,
        lease: &mut Option<ipp_core::services::gui_input::GuiPointerLease>,
        update: ipp_core::systems::gui::local::GuiInteractionUpdate,
    ) {
        self.request += 1;
        let target = self.read(self.control).unwrap().target;
        let input = self
            .input
            .reserve_routed(
                &self.host,
                context,
                target,
                self.request,
                &[],
                Box::new(Applied),
            )
            .unwrap();
        let lease = lease
            .get_or_insert_with(|| self.input.pointer_lease(&input, 1).unwrap())
            .clone();
        let command = GuiLocalCommand::interaction(input, lease, update).unwrap();
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue_system_command(GuiSystem::ID, 800, command)
            .unwrap();
    }

    /// The control's stored control component.
    fn control_component(&mut self) -> ComponentValue {
        self.host
            .world_mut(self.world)
            .unwrap()
            .inspect(self.control)
            .unwrap()
            .components
            .into_iter()
            .find(|value| {
                matches!(
                    value,
                    ComponentValue::GuiButton(_)
                        | ComponentValue::GuiCheckbox(_)
                        | ComponentValue::GuiSlider(_)
                        | ComponentValue::GuiTextInput(_)
                        | ComponentValue::GuiScrollView(_)
                        | ComponentValue::GuiVirtualList(_)
                )
            })
            .unwrap()
    }
}

#[test]
fn pointer_hover_press_release_and_cancel_drive_continuous_skin_motion() {
    use ipp_core::systems::gui::local::GuiInteractionUpdate;

    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    fixture.with_state_rows(&[
        (
            GuiPartId::state(GuiPrimitivePart::Background, GuiSkinState::Hovered),
            PURPLE,
            0.5,
        ),
        (
            GuiPartId::state(GuiPrimitivePart::Background, GuiSkinState::Pressed),
            VIOLET,
            0.75,
        ),
    ]);
    let context = fixture.pointer_context();
    assert_eq!(fixture.color(), RED);
    let idle_control = fixture.control_component();
    let mut lease = None;

    // Hover starts a sampled transition toward the hovered destination.
    fixture.pointer(&context, &mut lease, GuiInteractionUpdate::Hover(true));
    fixture.frame(0.25);
    assert_color_near(fixture.color(), lerp(RED, PURPLE, 0.25));
    fixture.frame(0.75);
    assert_color_near(fixture.color(), PURPLE);

    // Press interrupts from the hovered composite toward the pressed destination.
    fixture.pointer(&context, &mut lease, GuiInteractionUpdate::Press);
    fixture.frame(0.25);
    let pressing = lerp(PURPLE, VIOLET, 0.25);
    assert_color_near(fixture.color(), pressing);

    // Release returns toward hovered from the actual composite, not an endpoint.
    fixture.pointer(&context, &mut lease, GuiInteractionUpdate::Release);
    fixture.frame(0.5);
    let releasing = lerp(pressing, PURPLE, 0.5);
    assert_color_near(fixture.color(), releasing);

    // A second press interrupts the release, and cancellation returns to idle.
    fixture.pointer(&context, &mut lease, GuiInteractionUpdate::Press);
    fixture.frame(0.25);
    let repressing = lerp(releasing, VIOLET, 0.25);
    assert_color_near(fixture.color(), repressing);
    fixture.pointer(&context, &mut lease, GuiInteractionUpdate::Cancel);
    fixture.frame(0.5);
    assert_color_near(fixture.color(), lerp(repressing, RED, 0.5));
    fixture.frame(0.5);
    assert_color_near(fixture.color(), RED);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));

    // Pointer feedback never writes the control.
    assert_eq!(fixture.control_component(), idle_control);
}

#[test]
fn interrupted_disable_and_hover_chain_settles_on_each_destination() {
    use ipp_core::systems::gui::local::GuiInteractionUpdate;

    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    fixture.with_state_rows(&[(
        GuiPartId::state(GuiPrimitivePart::Background, GuiSkinState::Hovered),
        PURPLE,
        0.5,
    )]);
    let context = fixture.pointer_context();
    let mut lease = None;
    fixture.pointer(&context, &mut lease, GuiInteractionUpdate::Hover(true));
    fixture.frame(1.0);
    assert_color_near(fixture.color(), PURPLE);

    // Disabling cancels hover and moves toward the disabled destination.
    fixture.enabled(false);
    fixture.frame(0.25);
    let disabling = lerp(PURPLE, BLUE, 0.25);
    assert_color_near(fixture.color(), disabling);

    // Re-enabling mid-way returns toward idle, since the hover was cancelled.
    fixture.enabled(true);
    fixture.frame(0.5);
    let enabling = lerp(disabling, RED, 0.5);
    assert_color_near(fixture.color(), enabling);

    // Disabling again during the return starts from that composite.
    fixture.enabled(false);
    fixture.frame(0.25);
    assert_color_near(fixture.color(), lerp(enabling, BLUE, 0.25));
    fixture.frame(1.0);
    assert_color_near(fixture.color(), BLUE);

    // A new hover after re-enabling settles on the hovered destination.
    fixture.enabled(true);
    fixture.frame(1.0);
    assert_color_near(fixture.color(), RED);
    let mut lease = None;
    fixture.pointer(&context, &mut lease, GuiInteractionUpdate::Hover(true));
    fixture.frame(1.0);
    assert_color_near(fixture.color(), PURPLE);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
}

#[test]
fn mismatched_clip_endpoints_paint_the_authored_appearance_statically_until_corrected() {
    // Disabled samples the clip at 0.5 (purple) but authors blue: the
    // destination cannot bind, so the authored value paints without motion.
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let (appearance, mut motion) = declarations(SOURCE);
    let disabled = GuiPartId::state(GuiPrimitivePart::Background, GuiSkinState::Disabled)
        .index()
        .unwrap();
    for (_, motion_row) in motion.parts.iter_mut() {
        if motion_row.part == disabled {
            motion_row.time = Some(0.5);
        }
    }
    apply(
        &mut fixture.host,
        fixture.world,
        vec![Command::insert_value(
            EntityRef::Handle(fixture.theme),
            ComponentValue::GuiThemeMotion(motion),
        )],
    )
    .result
    .unwrap();
    fixture.enabled(false);
    fixture.frame(0.25);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    assert_eq!(fixture.color(), BLUE);
    fixture.frame(0.25);
    assert_eq!(fixture.color(), BLUE);

    // Correcting the sample time recovers ordinary motion for the same control.
    let (_, corrected) = declarations(SOURCE);
    apply(
        &mut fixture.host,
        fixture.world,
        vec![
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiTheme(appearance),
            ),
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiThemeMotion(corrected),
            ),
        ],
    )
    .result
    .unwrap();
    fixture.frame(0.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    fixture.enabled(true);
    fixture.frame(0.25);
    assert_color_near(fixture.color(), lerp(BLUE, RED, 0.25));
}

#[test]
fn editing_the_destination_appearance_mid_transition_keeps_the_authored_value() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let disabled = GuiPartId::state(GuiPrimitivePart::Background, GuiSkinState::Disabled)
        .index()
        .unwrap();
    let theme = |fixture: &mut Fixture| {
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .inspect(fixture.theme)
            .unwrap()
            .components
            .into_iter()
            .find_map(|component| match component {
                ComponentValue::GuiTheme(theme) => Some(theme),
                _ => None,
            })
            .unwrap()
    };
    let slot = theme(&mut fixture)
        .parts
        .iter()
        .find(|(_, row)| row.part == disabled)
        .map(|(slot, _)| slot)
        .unwrap();
    let theme_entity = fixture.theme;
    let write_color = |color: [f32; 4]| Command::SetField {
        entity: EntityRef::Handle(theme_entity),
        component: ComponentValue::GUI_THEME,
        field: FieldWrite {
            offset: Rows::<GuiPaintPart>::offset(
                0,
                slot,
                ipp_core::systems::gui::GuiPartProperty::Color.index(),
            )
            .unwrap(),
            value: FieldValue::Dynamic(DynamicValue::Vec4(color)),
        },
    };
    fixture.enabled(false);
    fixture.frame(0.25);
    assert_color_near(fixture.color(), lerp(RED, BLUE, 0.25));

    // A streamed edit makes the destination disagree with the clip sample: the
    // authored value paints statically and is never overwritten by motion.
    apply(&mut fixture.host, fixture.world, vec![write_color(GREEN)])
        .result
        .unwrap();
    fixture.frame(0.25);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    assert_eq!(fixture.color(), GREEN);
    fixture.frame(0.25);
    assert_eq!(fixture.color(), GREEN);
    let authored = theme(&mut fixture);
    let (_, row) = authored
        .parts
        .iter()
        .find(|(_, row)| row.part == disabled)
        .unwrap();
    assert_eq!(row.color, Some(GREEN));

    // Restoring agreement with the clip recovers motion and settles on it.
    apply(&mut fixture.host, fixture.world, vec![write_color(BLUE)])
        .result
        .unwrap();
    fixture.frame(0.0);
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    fixture.frame(1.0);
    assert_eq!(fixture.color(), BLUE);
}

#[test]
fn terminal_clip_reload_failure_withdraws_motion_to_the_static_destination() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    fixture.enabled(false);
    fixture.frame(0.25);
    assert_color_near(fixture.color(), lerp(RED, BLUE, 0.25));
    let key = fixture.key();
    fixture.host.asset_resources_mut().unload(key);
    fixture.host.flush_resource_lifecycle();
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Pending));
    let mut failed = false;
    let mut playback_events = Vec::new();
    for _ in 0..16 {
        for request in fixture.host.take_resource_requests() {
            assert_eq!(request.source, std::sync::Arc::<str>::from(SOURCE));
            fixture
                .host
                .complete_resource(request.id, Err("InvalidAsset".into()))
                .unwrap();
            failed = true;
        }
        let frame = fixture.host.frame(0.0).unwrap();
        for report in frame.worlds.into_values() {
            playback_events.extend(report.unwrap().playback_events);
        }
        if failed && fixture.status() == Some(GuiSkinMotionStatus::Unavailable) {
            break;
        }
    }
    assert!(failed, "the unloaded clip must be requested again");
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Unavailable));
    // Motion is private: no ordinary playback event reports it.
    assert!(playback_events.is_empty(), "{playback_events:?}");
    fixture.frame(0.25);
    assert_eq!(fixture.color(), BLUE);
    assert!(
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .animation_controllers()
            .is_empty()
    );
}

#[test]
fn live_controller_restore_neither_captures_nor_disturbs_active_skin_motion() {
    use ipp_core::systems::animation::AnimationPersistentState;

    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    fixture.enabled(false);
    fixture.frame(0.25);
    let first = lerp(RED, BLUE, 0.25);
    assert_color_near(fixture.color(), first);
    let persistent = fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .animation_persistent_state();
    assert!(persistent.controllers.is_empty());

    // A refused replacement and a successful one both leave motion running.
    assert_eq!(
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .restore_animation_controllers(AnimationPersistentState {
                next_id: 0,
                ..Default::default()
            }),
        Err(ErrorReason::InvalidValue)
    );
    fixture
        .host
        .world_mut(fixture.world)
        .unwrap()
        .restore_animation_controllers(persistent.clone())
        .unwrap();
    fixture.frame(0.25);
    assert_color_near(fixture.color(), lerp(RED, BLUE, 0.5));
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    fixture.frame(0.5);
    assert_eq!(fixture.color(), BLUE);
    assert_eq!(
        fixture
            .host
            .world_mut(fixture.world)
            .unwrap()
            .animation_persistent_state(),
        persistent
    );
}

#[test]
fn unrelated_edits_every_frame_leave_a_shared_clip_transition_undisturbed() {
    let mut fixture = Fixture::new(ComponentValue::GuiButton(GuiButton::default()));
    let parent = fixture.canvas;
    let sibling = create(
        &mut fixture.host,
        fixture.world,
        vec![
            ComponentValue::GuiCheckbox(GuiCheckbox::default()),
            ComponentValue::GuiLayout(GuiLayout {
                width: 20.0,
                height: 20.0,
                ..Default::default()
            }),
        ],
        Some(parent),
    );
    let bystander = create(
        &mut fixture.host,
        fixture.world,
        vec![ComponentValue::CanvasStyle(Default::default())],
        None,
    );
    fixture.frame(0.0);
    let toggle_sibling = |fixture: &mut Fixture| {
        let control = fixture.control;
        fixture.control = sibling;
        fixture.action(GuiLocalAction::Toggle);
        fixture.control = control;
    };

    // Disabled and idle share one clip; every frame also commits a sibling
    // control and streams an unrelated field edit.
    fixture.enabled(false);
    let mut expected = RED;
    for step in 1..=4 {
        apply(
            &mut fixture.host,
            fixture.world,
            vec![Command::SetField {
                entity: EntityRef::Handle(bystander),
                component: ComponentValue::CANVAS_STYLE,
                field: FieldWrite {
                    offset: std::mem::offset_of!(ipp_core::systems::canvas::CanvasStyle, x) as u32,
                    value: FieldValue::F32(step as f32),
                },
            }],
        )
        .result
        .unwrap();
        // The sibling action is queued after the applied edit.
        toggle_sibling(&mut fixture);
        fixture.frame(0.25);
        expected = lerp(RED, BLUE, step as f32 * 0.25);
        assert_color_near(fixture.color(), expected);
        assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    }
    assert_eq!(expected, BLUE);

    // Re-enabling returns through the same clip while the edits continue.
    fixture.enabled(true);
    for step in 1..=4 {
        toggle_sibling(&mut fixture);
        fixture.frame(0.25);
        assert_color_near(fixture.color(), lerp(BLUE, RED, step as f32 * 0.25));
    }
    assert_eq!(fixture.status(), Some(GuiSkinMotionStatus::Ready));
    // Eight toggles, each settled as applied, return the sibling to unchecked.
    assert_eq!(
        fixture.read(sibling).unwrap().value,
        ControlValue::Bool(false)
    );
}

#[test]
fn sampled_colour_reaches_solid_fill_and_focus_stroke_but_not_implicit_gradient_stops() {
    use ipp_core::systems::canvas::CanvasPart;

    // Checked restyles both the background and the focus ring through the
    // shared clip; the background additionally declares a linear gradient
    // whose first stop is implicit.
    let mut fixture = Fixture::new(ComponentValue::GuiCheckbox(GuiCheckbox::default()));
    let (mut appearance, mut motion) = declarations(SOURCE);
    for (_, row) in appearance.parts.iter_mut() {
        if row.part
            == GuiPartId::base(GuiPrimitivePart::Background)
                .index()
                .unwrap()
        {
            row.fill_mode = Some(1.0);
            row.gradient_color1 = Some(GREEN);
        }
    }
    for (part, color, time) in [
        (GuiPartId::base(GuiPrimitivePart::FocusRing), RED, 0.0),
        (
            GuiPartId::variant(
                GuiPrimitivePart::FocusRing,
                GuiSkinState::Idle,
                GuiPartVariant::Checked,
            ),
            BLUE,
            1.0,
        ),
    ] {
        appearance
            .parts
            .push(GuiPaintPart {
                color: Some(color),
                opacity: Some(1.0),
                scale: Some([1.0; 2]),
                ..GuiPaintPart::keyed(part).unwrap()
            })
            .unwrap();
        motion
            .parts
            .push(GuiMotionPart {
                part: part.index().unwrap(),
                source: Some(source(SOURCE)),
                duration: Some(1.0),
                easing: Some(0),
                track: Some(0),
                time: Some(time),
            })
            .unwrap();
    }
    apply(
        &mut fixture.host,
        fixture.world,
        vec![
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiTheme(appearance),
            ),
            Command::insert_value(
                EntityRef::Handle(fixture.theme),
                ComponentValue::GuiThemeMotion(motion),
            ),
        ],
    )
    .result
    .unwrap();
    fixture.action(GuiLocalAction::Focus);
    fixture.frame(1.0);
    fixture.frame(0.0);
    let parts = |fixture: &Fixture| {
        let publication = fixture.publication();
        let find = |part| {
            publication
                .entries
                .iter()
                .find_map(|entry| match entry.as_ref() {
                    CanvasPaintEntry::Primitive {
                        primitive:
                            CanvasPrimitive::Box {
                                style,
                                fill,
                                border_color,
                                ..
                            },
                        ..
                    } if style.identity.target.entity == fixture.control
                        && style.identity.part == part =>
                    {
                        Some((*fill, *border_color))
                    }
                    _ => None,
                })
                .unwrap()
        };
        (
            find(CanvasPart::Background).0,
            find(CanvasPart::FocusRing).1,
        )
    };
    let gradient = |start| CanvasShapeFill::LinearGradient {
        start: [0.0, 0.0],
        end: [1.0, 1.0],
        start_color: start,
        end_color: GREEN,
    };
    assert_eq!(parts(&fixture), (gradient(RED), RED));

    fixture.action(GuiLocalAction::Toggle);
    fixture.frame(0.25);
    let sampled = lerp(RED, BLUE, 0.25);
    let (fill, stroke) = parts(&fixture);
    // The focus stroke observes the sampled colour...
    assert_color_near(stroke, sampled);
    // ...while gradient stops are unanimated material: the implicit first stop
    // takes the destination colour, not the mid-transition sample.
    assert_eq!(fill, gradient(BLUE));
    fixture.frame(1.0);
    assert_eq!(parts(&fixture), (gradient(BLUE), BLUE));
}
