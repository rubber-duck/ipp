//! Skin transitions under interruption, streamed lane edits, node and root
//! removal, and asset progression between input admission and evaluation.
//! Every destination shares one motion clip that animates the idle
//! `background` lanes, like the gallery skins, and every frame commits a
//! second control's value through GUI input, like a dragged slider.

use super::*;

const FRAME: f64 = 1.0 / 60.0;

const IDLE: SkinMotionSample = (0.0, [0.38, 0.85, 1.0, 0.9], 1.0, [1.0, 1.0]);

const HOVERED: SkinMotionSample = (0.1, [0.78, 0.96, 1.0, 1.0], 1.0, [1.025, 1.025]);

const PRESSED: SkinMotionSample = (0.2, [0.2, 0.65, 0.88, 0.9], 1.0, [0.985, 0.985]);

const DISABLED: SkinMotionSample = (0.3, [0.2, 0.35, 0.42, 0.45], 0.45, [1.0, 1.0]);

const SAMPLES: [SkinMotionSample; 5] = [
    IDLE,
    HOVERED,
    PRESSED,
    DISABLED,
    (0.4, [0.2, 0.8, 0.94, 1.0], 1.0, [1.0, 1.0]),
];

/// Node 2 skin lanes authored from the shared clip's samples.
fn shared_skin_root(source: &AssetSource) -> GuiRoot {
    let mut root = GuiRoot::default();
    for (part, sample) in [
        ("background", IDLE),
        ("background_hovered", HOVERED),
        ("background_pressed", PRESSED),
        ("background_disabled", DISABLED),
    ] {
        part_color(&mut root, 2, part, sample.1);
        part_f32(&mut root, 2, part, "opacity", sample.2);
        part_vec2(&mut root, 2, part, "scale", sample.3);
        part_motion(&mut root, 2, part, source.clone());
        part_f32(&mut root, 2, part, "duration", 0.2);
        part_f32(&mut root, 2, part, "easing", 0.0);
        part_f32(&mut root, 2, part, "track", 0.0);
        part_f32(&mut root, 2, part, "time", sample.0 as f32);
    }
    root
}

fn shared_clip_bytes() -> Vec<u8> {
    shared_skin_motion_clip(&SAMPLES).encode()
}

/// A panel whose shared clip is already decoded and idle.
fn ready_skin_panel(name: &str) -> (HostRuntime, WorldId, crate::EntityId) {
    let source = skin_motion_source(name);
    let (mut host, world, panel) = skin_panel_with_root(shared_skin_root(&source));
    insert_committing_control(&mut host, world, panel);
    host.asset_resources_mut()
        .register_client_source(world, source, shared_clip_bytes())
        .unwrap();
    for _ in 0..32 {
        host.progress_assets();
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }
    assert_color_near(panel_box_color(&mut host, world, panel), IDLE.1);
    (host, world, panel)
}

/// Insert the column, the skinned checkbox and the committing checkbox of a
/// fresh root incarnation.
fn insert_skin_tree(host: &mut HostRuntime, world: WorldId, panel: crate::EntityId) {
    let mut context = host.world_mut(world).unwrap();
    let incarnation = context
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    let checkbox = GuiNodeContent::Checkbox {
        checked: false,
    };
    for (id, parent, index, content) in [
        (
            1,
            None,
            0,
            GuiNodeContent::Container(GuiContainerKind::Column),
        ),
        (2, Some(GuiNodeId(1)), 0, checkbox.clone()),
        (3, Some(GuiNodeId(1)), 1, checkbox),
    ] {
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::InsertNode {
                    entity: panel,
                    root_incarnation: incarnation,
                    id: GuiNodeId(id),
                    parent,
                    index,
                    content,
                    style: GuiNodeStyle {
                        width: Some(if id == 1 {
                            10.0
                        } else {
                            2.0
                        }),
                        height: Some(if id == 1 {
                            10.0
                        } else {
                            1.0
                        }),
                        ..Default::default()
                    },
                },
            )
            .unwrap();
    }
    context.step(0.0).unwrap();
}

/// A second checkbox whose routed clicks commit a control value, like a
/// dragged slider, on frames whose skin output is retained.
fn insert_committing_control(host: &mut HostRuntime, world: WorldId, panel: crate::EntityId) {
    let mut context = host.world_mut(world).unwrap();
    let incarnation = context
        .inspect_gui(panel, None, 1, 1)
        .unwrap()
        .root_incarnation;
    context
        .enqueue_gui_command(
            SESSION,
            GuiCommand::InsertNode {
                entity: panel,
                root_incarnation: incarnation,
                id: GuiNodeId(3),
                parent: Some(GuiNodeId(1)),
                index: 1,
                content: GuiNodeContent::Checkbox {
                    checked: false,
                },
                style: GuiNodeStyle {
                    width: Some(2.0),
                    height: Some(1.0),
                    ..Default::default()
                },
            },
        )
        .unwrap();
    context.step(0.0).unwrap();
}

/// Queue a click on the committing checkbox for the next frame.
fn commit_other_control(host: &mut HostRuntime, world: WorldId, panel: crate::EntityId) {
    let at = node_centre(host, world, panel, GuiNodeId(3));
    let mut context = host.world_mut(world).unwrap();
    context
        .enqueue_gui_input_command(SESSION, pointer_down(9, at))
        .unwrap();
    context
        .enqueue_gui_input_command(SESSION, pointer_up(9, at))
        .unwrap();
}

fn committed_clicks(host: &mut HostRuntime, world: WorldId, panel: crate::EntityId) -> u32 {
    host.world_mut(world)
        .unwrap()
        .inspect_gui(panel, Some(GuiNodeId(3)), 1, 4)
        .unwrap()
        .nodes
        .into_iter()
        .find(|node| node.id == GuiNodeId(3))
        .unwrap()
        .control_revision
}

/// Step whole input frames, each committing the other control, refusing any
/// skin transition rejection, and return the painted background per frame.
fn frames(
    host: &mut HostRuntime,
    world: WorldId,
    panel: crate::EntityId,
    count: usize,
) -> Vec<[f32; 4]> {
    let owner = background_skin_owner(host, world, panel);
    (0..count)
        .map(|_| {
            commit_other_control(host, world, panel);
            host.world_mut(world).unwrap().step(FRAME).unwrap();
            assert_eq!(skin_controller_refused(host, world, owner), None);
            panel_box_color(host, world, panel)
        })
        .collect()
}

fn hover(host: &mut HostRuntime, world: WorldId, panel: crate::EntityId, inside: bool) {
    let at = if inside {
        node_centre(host, world, panel, GuiNodeId(2))
    } else {
        [9.0, 9.0]
    };
    host.world_mut(world)
        .unwrap()
        .enqueue_gui_input_command(SESSION, pointer_move(3, at))
        .unwrap();
}

fn assert_continuous(from: [f32; 4], to: [f32; 4]) {
    for (from, to) in from.into_iter().zip(to) {
        assert!(
            (from - to).abs() <= 0.1,
            "interruption jumped from {from} to {to}"
        );
    }
}

/// A transition in progress has left `from` without reaching `to`.
fn assert_between(paint: [f32; 4], from: SkinMotionSample, to: SkinMotionSample) {
    let distance = |sample: SkinMotionSample| {
        paint
            .iter()
            .zip(sample.1)
            .map(|(value, target)| (value - target).abs())
            .fold(0.0, f32::max)
    };
    assert!(
        distance(from) > 0.02 && distance(to) > 0.02,
        "{paint:?} is not between {:?} and {:?}",
        from.1,
        to.1
    );
}

/// The paint and effective idle lanes rest on `state`; the authored idle
/// lanes stay the idle sample.
fn assert_settled(
    host: &mut HostRuntime,
    world: WorldId,
    panel: crate::EntityId,
    state: SkinMotionSample,
) {
    assert_color_near(panel_box_color(host, world, panel), state.1);
    let [base, effective] = idle_background_lanes(host, world, panel);
    assert_eq!(
        (base.color, base.opacity, base.scale),
        (Some(IDLE.1), Some(IDLE.2), Some(IDLE.3))
    );
    assert_color_near(effective.color.unwrap(), state.1);
    assert!((effective.opacity.unwrap() - state.2).abs() <= 1.0e-5);
    let scale = effective.scale.unwrap();
    assert!((scale[0] - state.3[0]).abs() <= 1.0e-5 && (scale[1] - state.3[1]).abs() <= 1.0e-5);
}

#[test]
fn interrupted_disable_and_hover_chain_settles_on_each_destination() {
    let (mut host, world, panel) = ready_skin_panel("skin-chain");
    set_node_enabled(&mut host, world, panel, false);
    frames(&mut host, world, panel, 24);
    assert_settled(&mut host, world, panel, DISABLED);

    // Re-disable partway through disabled -> idle. The new request starts
    // from the in-flight composite rather than either endpoint.
    set_node_enabled(&mut host, world, panel, true);
    let partway = *frames(&mut host, world, panel, 6).last().unwrap();
    assert_between(partway, DISABLED, IDLE);
    set_node_enabled(&mut host, world, panel, false);
    let resumed = frames(&mut host, world, panel, 24);
    assert_continuous(partway, resumed[0]);
    assert_settled(&mut host, world, panel, DISABLED);

    // Hover partway through disabled -> idle; hover input arrives on a frame
    // whose skin output is in flight.
    set_node_enabled(&mut host, world, panel, true);
    let partway = *frames(&mut host, world, panel, 6).last().unwrap();
    assert_between(partway, DISABLED, IDLE);
    hover(&mut host, world, panel, true);
    let hovered = frames(&mut host, world, panel, 24);
    assert_continuous(partway, hovered[0]);
    assert_settled(&mut host, world, panel, HOVERED);

    // Hover -> idle, re-disabled partway through the return to idle.
    hover(&mut host, world, panel, false);
    let partway = *frames(&mut host, world, panel, 6).last().unwrap();
    assert_between(partway, HOVERED, IDLE);
    set_node_enabled(&mut host, world, panel, false);
    let disabled = frames(&mut host, world, panel, 24);
    assert_continuous(partway, disabled[0]);
    assert_settled(&mut host, world, panel, DISABLED);

    set_node_enabled(&mut host, world, panel, true);
    frames(&mut host, world, panel, 24);
    assert_settled(&mut host, world, panel, IDLE);
    hover(&mut host, world, panel, true);
    frames(&mut host, world, panel, 24);
    assert_settled(&mut host, world, panel, HOVERED);
    hover(&mut host, world, panel, false);
    frames(&mut host, world, panel, 24);
    assert_settled(&mut host, world, panel, IDLE);

    // Most of the 234 input frames committed the other checkbox; a click
    // admitted beside other ingress may commit on the following frame.
    let clicks = committed_clicks(&mut host, world, panel);
    assert!(clicks > 150, "only {clicks} committed clicks");
}

#[test]
fn streamed_edit_of_the_animated_idle_opacity_mid_transition_keeps_the_authored_value() {
    let (mut host, world, panel) = ready_skin_panel("skin-streamed-opacity");
    set_node_enabled(&mut host, world, panel, false);
    frames(&mut host, world, panel, 24);
    set_node_enabled(&mut host, world, panel, true);
    let partway = *frames(&mut host, world, panel, 6).last().unwrap();
    assert_between(partway, DISABLED, IDLE);

    // A Host command chunk edits the idle opacity lane the in-flight
    // transition animates. It stages the authored value, never the
    // retained transition output.
    let opacity = GuiRoot::part_property_name(GuiNodeId(2), "background", "opacity").unwrap();
    {
        let mut context = host.world_mut(world).unwrap();
        // Like the Host, defer the chunk while earlier ingress drains.
        let mut deferred = 0;
        while context.has_deferred_world_input() {
            context.step(0.0).unwrap();
            deferred += 1;
            assert!(deferred < 4, "ingress did not drain");
        }
        let outcome = context
            .apply_command_chunk(Batch {
                id: 60,
                operations: vec![Command::SetDynamicProperty {
                    entity: EntityRef::Handle(panel),
                    component: ComponentValue::GUI_ROOT,
                    name: opacity,
                    value: DynamicValue::F32(0.8),
                }],
            })
            .unwrap();
        assert!(outcome.result.is_ok());
        context.finish_command_stream();
    }
    for _ in 0..30 {
        host.world_mut(world).unwrap().step(FRAME).unwrap();
        let [base, _] = idle_background_lanes(&mut host, world, panel);
        assert_eq!(base.opacity, Some(0.8));
        assert_eq!(base.color, Some(IDLE.1));
    }

    // The edited idle lane no longer matches the clip's idle sample, so the
    // returned idle state presents the authored lanes.
    let [base, effective] = idle_background_lanes(&mut host, world, panel);
    assert_eq!(base.opacity, Some(0.8));
    assert!((effective.opacity.unwrap() - 0.8).abs() <= 1.0e-5);
    assert_color_near(effective.color.unwrap(), IDLE.1);
    assert_color_near(panel_box_color(&mut host, world, panel), IDLE.1);
}

#[test]
fn removing_the_node_or_root_mid_transition_withdraws_before_the_slot_is_reused() {
    let (mut host, world, panel) = ready_skin_panel("skin-removal");
    let gui_roots = |host: &mut HostRuntime, panel| {
        let inspected = host.world_mut(world).unwrap().inspect(panel).unwrap();
        let root = |values: Vec<ComponentValue>| {
            values.into_iter().find_map(|value| match value {
                ComponentValue::GuiRoot(root) => Some(root),
                _ => None,
            })
        };
        (root(inspected.base), root(inspected.effective))
    };

    // Node removal mid-transition withdraws the private controller and its
    // retained output; the remaining root presents its authored values.
    set_node_enabled(&mut host, world, panel, false);
    assert_between(
        *frames(&mut host, world, panel, 6).last().unwrap(),
        IDLE,
        DISABLED,
    );
    let owner = background_skin_owner(&mut host, world, panel);
    {
        let mut context = host.world_mut(world).unwrap();
        let incarnation = context
            .inspect_gui(panel, None, 1, 1)
            .unwrap()
            .root_incarnation;
        context
            .enqueue_gui_command(
                SESSION,
                GuiCommand::RemoveNode {
                    handle: GuiNodeHandle::new(SESSION, panel, incarnation, GuiNodeId(2), 1),
                },
            )
            .unwrap();
        context.step(FRAME).unwrap();
        context.step(FRAME).unwrap();
    }
    assert!(skin_controller_snapshot(&mut host, world, owner).is_none());
    let (base, effective) = gui_roots(&mut host, panel);
    assert_eq!(base, effective);

    // Root removal mid-transition, then a fresh root incarnation on the same
    // entity reuses node 2 and transitions from its own authored lanes.
    let source = skin_motion_source("skin-removal");
    for round in 0..2 {
        {
            let mut context = host.world_mut(world).unwrap();
            context
                .enqueue(Batch {
                    id: 70 + round * 2,
                    operations: vec![Command::RemoveComponent {
                        entity: EntityRef::Handle(panel),
                        component: ComponentValue::GUI_ROOT,
                    }],
                })
                .unwrap();
            context.step(FRAME).unwrap();
            context
                .enqueue(Batch {
                    id: 71 + round * 2,
                    operations: vec![Command::InsertComponentValue {
                        entity: EntityRef::Handle(panel),
                        value: ComponentValue::GuiRoot(shared_skin_root(&source)),
                    }],
                })
                .unwrap();
            context.step(FRAME).unwrap();
        }
        assert!(skin_controller_snapshot(&mut host, world, owner).is_none());
        insert_skin_tree(&mut host, world, panel);
        frames(&mut host, world, panel, 4);
        assert_settled(&mut host, world, panel, IDLE);
        set_node_enabled(&mut host, world, panel, false);
        assert_between(
            *frames(&mut host, world, panel, 6).last().unwrap(),
            IDLE,
            DISABLED,
        );
    }
    frames(&mut host, world, panel, 24);
    assert_settled(&mut host, world, panel, DISABLED);
    set_node_enabled(&mut host, world, panel, true);
    frames(&mut host, world, panel, 24);
    assert_settled(&mut host, world, panel, IDLE);

    // Deleting the entity mid-transition and creating a replacement panel,
    // which may reuse its storage slot, starts without inherited output.
    set_node_enabled(&mut host, world, panel, false);
    frames(&mut host, world, panel, 6);
    let replacement = {
        let mut context = host.world_mut(world).unwrap();
        context
            .enqueue(Batch {
                id: 80,
                operations: vec![
                    Command::Delete {
                        entity: EntityRef::Handle(panel),
                    },
                    Command::Create {
                        alias: 1,
                        metadata: EntityMetadata::default(),
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Alias(1),
                        value: ComponentValue::Surface({
                            let mut surface = Surface::default();
                            surface.width = 10.0;
                            surface.height = 10.0;
                            surface
                        }),
                    },
                    Command::InsertComponentValue {
                        entity: EntityRef::Alias(1),
                        value: ComponentValue::GuiRoot(shared_skin_root(&source)),
                    },
                ],
            })
            .unwrap();
        let report = context.step(FRAME).unwrap();
        report.outcomes[0].result.as_ref().unwrap()[0].1
    };
    assert_ne!(replacement, panel);
    insert_skin_tree(&mut host, world, replacement);
    frames(&mut host, world, replacement, 4);
    assert_settled(&mut host, world, replacement, IDLE);
    let (base, effective) = gui_roots(&mut host, replacement);
    assert_eq!(base, effective);
    set_node_enabled(&mut host, world, replacement, false);
    frames(&mut host, world, replacement, 24);
    assert_settled(&mut host, world, replacement, DISABLED);
    set_node_enabled(&mut host, world, replacement, true);
    frames(&mut host, world, replacement, 24);
    assert_settled(&mut host, world, replacement, IDLE);
}

/// Host frame order with GUI input: admission (and its restore) precedes
/// shared asset progression, which precedes evaluation.
fn input_frame_with_assets(
    host: &mut HostRuntime,
    world: WorldId,
    panel: crate::EntityId,
    between: impl FnOnce(&mut HostRuntime),
) {
    commit_other_control(host, world, panel);
    host.world_mut(world)
        .unwrap()
        .prepare_update(FRAME)
        .unwrap();
    between(host);
    host.progress_assets();
    host.world_mut(world).unwrap().step(FRAME).unwrap();
}

fn next_request(host: &mut HostRuntime, world: WorldId) -> u64 {
    for _ in 0..16 {
        host.progress_assets();
        if let Some(request) = host
            .world_mut(world)
            .unwrap()
            .take_resource_requests()
            .into_iter()
            .next()
        {
            return request.id;
        }
        host.world_mut(world).unwrap().step(0.0).unwrap();
    }
    panic!("the skin clip was not requested");
}

#[test]
fn skin_clip_completing_or_suspending_between_input_admission_and_step() {
    let source = AssetSource {
        kind: ANIMATION_TYPE,
        uri: "https://skins.test/shared.ippa".into(),
        variant: 0,
    };
    let (mut host, world, panel) = skin_panel_with_root(shared_skin_root(&source));
    insert_committing_control(&mut host, world, panel);
    host.world_mut(world)
        .unwrap()
        .register_stream_resource_provider("https")
        .unwrap();
    let request = next_request(&mut host, world);
    assert_color_near(panel_box_color(&mut host, world, panel), IDLE.1);

    // The clip completes after hover input is admitted and restored but
    // before the frame evaluates.
    hover(&mut host, world, panel, true);
    input_frame_with_assets(&mut host, world, panel, |host| {
        host.world_mut(world)
            .unwrap()
            .complete_resource(request, Ok(shared_clip_bytes()))
            .unwrap();
    });
    frames(&mut host, world, panel, 30);
    assert_settled(&mut host, world, panel, HOVERED);
    hover(&mut host, world, panel, false);
    let returning = frames(&mut host, world, panel, 30);
    assert!(
        returning.iter().any(|paint| {
            paint
                .iter()
                .zip(HOVERED.1.iter().zip(IDLE.1))
                .all(|(value, (from, to))| {
                    from == &to || (value - from).abs() > 0.02 && (value - to).abs() > 0.02
                })
        }),
        "the ready clip did not blend hover -> idle"
    );
    assert_settled(&mut host, world, panel, IDLE);

    // The clip suspends while idle -> disabled is in flight, between an
    // input frame's admission and its evaluation.
    set_node_enabled(&mut host, world, panel, false);
    assert_between(
        *frames(&mut host, world, panel, 6).last().unwrap(),
        IDLE,
        DISABLED,
    );
    hover(&mut host, world, panel, false);
    input_frame_with_assets(&mut host, world, panel, |host| {
        let key = host.asset_resources().find(&source).unwrap();
        host.asset_resources_mut().unload(key);
    });
    let [base, _] = idle_background_lanes(&mut host, world, panel);
    assert_eq!(
        (base.color, base.opacity, base.scale),
        (Some(IDLE.1), Some(IDLE.2), Some(IDLE.3))
    );
    let recovery = next_request(&mut host, world);
    host.world_mut(world)
        .unwrap()
        .complete_resource(recovery, Ok(shared_clip_bytes()))
        .unwrap();
    host.progress_assets();
    frames(&mut host, world, panel, 30);
    assert_settled(&mut host, world, panel, DISABLED);
    set_node_enabled(&mut host, world, panel, true);
    frames(&mut host, world, panel, 30);
    assert_settled(&mut host, world, panel, IDLE);
}

#[test]
fn skin_transition_reconciles_and_repaints_on_every_animated_frame_only() {
    let (mut host, world, panel) = ready_skin_panel("skin-every-frame");
    host.world_mut(world).unwrap().step(FRAME).unwrap();
    take_preparation_counts();
    for _ in 0..4 {
        host.world_mut(world).unwrap().step(FRAME).unwrap();
    }
    assert_eq!(take_preparation_counts(), (4, 0));

    hover(&mut host, world, panel, true);
    let mut painted = vec![panel_box_color(&mut host, world, panel)];
    let mut reconciled = Vec::new();
    for _ in 0..40 {
        host.world_mut(world).unwrap().step(FRAME).unwrap();
        painted.push(panel_box_color(&mut host, world, panel));
        let (passes, reconciliations) = take_preparation_counts();
        assert_eq!(passes, 1, "one preparation per frame");
        reconciled.push(reconciliations);
    }

    // The 0.2 s crossfade repaints on every frame from its first sample to
    // its destination, and each of those frames reconciled the skin.
    let moving: Vec<usize> = (0..reconciled.len())
        .filter(|&frame| painted[frame + 1] != painted[frame])
        .collect();
    let (first, last) = (moving[0], *moving.last().unwrap());
    assert_eq!(moving, (first..=last).collect::<Vec<_>>());
    assert!(moving.len() >= 8, "only {} animated frames", moving.len());
    for (frame, count) in reconciled.iter().enumerate().take(last + 1).skip(first) {
        assert!(*count >= 1, "frame {frame} kept stale skin input");
    }

    assert_settled(&mut host, world, panel, HOVERED);
    assert!(
        reconciled[last + 3..].iter().all(|&count| count == 0),
        "settled frames still reconcile: {reconciled:?}"
    );
}
