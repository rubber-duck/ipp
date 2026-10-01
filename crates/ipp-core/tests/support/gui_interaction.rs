use super::*;
use ipp_core::services::gui_input::{GuiInputContext, GuiPointerLease};
use ipp_core::systems::gui::local::{GuiEntityTarget, GuiInteractionUpdate};

fn feedback(
    host: &mut ControlHost,
    context: &GuiInputContext,
    target: GuiEntityTarget,
    lease: &mut Option<GuiPointerLease>,
    update: GuiInteractionUpdate,
) {
    host.next_request += 1;
    let input = host
        .input
        .reserve_routed(
            &host.host,
            context,
            target,
            host.next_request,
            &[],
            Box::new(AppliedPermit),
        )
        .unwrap();
    let lease = lease
        .get_or_insert_with(|| host.input.pointer_lease(&input, 1).unwrap())
        .clone();
    let command = GuiLocalCommand::interaction(input, lease, update).unwrap();
    host.world_mut(target.world.id())
        .unwrap()
        .enqueue_system_command(GuiSystem::ID, 800, command)
        .unwrap();
}

#[test]
fn public_feedback_queue_publishes_all_priorities_without_reflow_or_repainting_identical_styles() {
    let (mut host, world, root) = fixture();
    let entity = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    host.set_root_output(
        root,
        WorldViewport {
            width: 300,
            height: 100,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    frame(&mut host);
    let context = host
        .input
        .bind_context(&host.host, &host.session, root.world())
        .unwrap()
        .context;
    let target = read_control(&mut host, world, entity).unwrap().target;
    let mut lease = None;
    let before = output(&host, root).0;
    let statistics = host
        .world_mut(world)
        .unwrap()
        .gui_entity_layout_statistics()
        .unwrap()
        .total;
    for (update, expected) in [
        (GuiInteractionUpdate::Hover(true), [true, false, false]),
        (GuiInteractionUpdate::Press, [true, true, false]),
        (GuiInteractionUpdate::Capture, [true, true, true]),
        (GuiInteractionUpdate::Release, [true, false, false]),
        (GuiInteractionUpdate::Hover(false), [false, false, false]),
    ] {
        feedback(&mut host, &context, target, &mut lease, update);
        frame(&mut host);
        let after = output(&host, root).0;
        assert_eq!(
            [
                after.interaction.hovered,
                after.interaction.pressed,
                after.interaction.captured
            ],
            expected
        );
        assert!(!after.interaction.focused);
        assert_eq!(
            after.interaction.requires_direct(),
            expected.into_iter().any(|value| value)
        );
        assert_eq!(after.paint_revision, before.paint_revision);
        assert_eq!(after.layout_revision, before.layout_revision);
        assert!(Arc::ptr_eq(&after.entries, &before.entries));
        assert_eq!(
            host.world_mut(world)
                .unwrap()
                .gui_entity_layout_statistics()
                .unwrap()
                .total,
            statistics
        );
    }
    assert_eq!(
        read_control(&mut host, world, entity).unwrap().value,
        ControlValue::None
    );
}

#[test]
fn ordinary_skin_feedback_preserves_state_precedence_and_control_override() {
    let (mut host, world, root) = fixture();
    let entity = control(
        &mut host,
        world,
        root,
        ComponentValue::GuiButton(GuiButton::default()),
    );
    let mut parts = Rows::default();
    for (state, color) in [
        (GuiSkinState::Idle, [0.1, 0.0, 0.0, 1.0]),
        (GuiSkinState::Hovered, [0.2, 0.0, 0.0, 1.0]),
        (GuiSkinState::Pressed, [0.3, 0.0, 0.0, 1.0]),
        (GuiSkinState::Disabled, [0.4, 0.0, 0.0, 1.0]),
    ] {
        parts
            .push(GuiPaintPart {
                color: Some(color),
                ..GuiPaintPart::keyed(GuiPartId::state(GuiPrimitivePart::Background, state))
                    .unwrap()
            })
            .unwrap();
    }
    let theme = create(
        &mut host,
        world,
        vec![ComponentValue::GuiTheme(GuiTheme {
            parts,
        })],
        None,
    );
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    host.set_root_output(
        root,
        WorldViewport {
            width: 300,
            height: 100,
            device_pixel_ratio: 1.0,
        },
    )
    .unwrap();
    frame(&mut host);
    let context = host
        .input
        .bind_context(&host.host, &host.session, root.world())
        .unwrap()
        .context;
    let target = read_control(&mut host, world, entity).unwrap().target;
    let mut lease = None;
    let red = |host: &HostRuntime| {
        let output = output(host, root).0;
        let CanvasPrimitive::Box {
            fill: CanvasShapeFill::Solid(color),
            ..
        } = primitive(&output.entries[0])
        else {
            panic!("expected solid background");
        };
        color[0]
    };
    assert_eq!(red(&host), 0.1);
    feedback(
        &mut host,
        &context,
        target,
        &mut lease,
        GuiInteractionUpdate::Hover(true),
    );
    frame(&mut host);
    assert_eq!(red(&host), 0.2);
    feedback(
        &mut host,
        &context,
        target,
        &mut lease,
        GuiInteractionUpdate::Press,
    );
    frame(&mut host);
    assert_eq!(red(&host), 0.3);
    let mut overrides = Rows::default();
    overrides
        .push(part(
            GuiPartId::base(GuiPrimitivePart::Background),
            [0.7, 0.0, 0.0, 1.0],
        ))
        .unwrap();
    apply(
        &mut host,
        world,
        vec![Command::insert_value(
            EntityRef::Handle(entity),
            ComponentValue::GuiSkin(GuiSkin {
                theme,
                parts: overrides,
                ..Default::default()
            }),
        )],
    )
    .result
    .unwrap();
    frame(&mut host);
    assert_eq!(red(&host), 0.7);
    apply(
        &mut host,
        world,
        vec![
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiSkin(GuiSkin {
                    theme,
                    ..Default::default()
                }),
            ),
            Command::insert_value(
                EntityRef::Handle(entity),
                ComponentValue::GuiBehavior(GuiBehavior {
                    enabled: false,
                    ..Default::default()
                }),
            ),
        ],
    )
    .result
    .unwrap();
    frame(&mut host);
    assert_eq!(red(&host), 0.4);
    assert!(!lease.unwrap().is_live());
    assert!(!output(&host, root).0.interaction.requires_direct());
}
