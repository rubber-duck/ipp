use crate::services::gui_input::test_support::*;
use crate::services::gui_input::{
    GuiDeliveryTerminal, GuiInputError, GuiInputLimits, GuiInputService,
};

#[test]
fn inert_cancelled_pointer_paths_and_slots_are_charged_through_last_drop() {
    let scene = scene();
    let command = routed(&scene);
    let first = scene.service.pointer_lease(&command, 9).unwrap();
    let second = scene.service.pointer_lease(&command, 9).unwrap();
    let retained = first.clone();
    assert_eq!(scene.service.registry.borrow().pointer_leases, 2);
    assert_eq!(scene.service.registry.borrow().pointer_slots.len(), 1);
    // One attachment token each for the command and both leases.
    assert_eq!(scene.service.registry.borrow().path_nodes, 3);
    scene.service.cancel_pointer_lease(&first).unwrap();
    assert!(!first.is_live() && !second.is_live());
    drop(command);
    assert_eq!(scene.service.registry.borrow().path_nodes, 2);
    drop(first);
    assert_eq!(scene.service.registry.borrow().pointer_leases, 2);
    drop(retained);
    assert_eq!(scene.service.registry.borrow().pointer_leases, 1);
    assert_eq!(scene.service.registry.borrow().pointer_slots.len(), 1);
    drop(second);
    assert_eq!(scene.service.registry.borrow().pointer_leases, 0);
    assert_eq!(scene.service.registry.borrow().pointer_slots.len(), 0);
    assert_eq!(scene.service.registry.borrow().path_nodes, 0);
}

#[test]
fn pointer_capacity_source_and_service_checks_never_admit_foreign_or_unbudgeted_candidates() {
    for limit in 0..2 {
        let mut scene = scene();
        scene.service = GuiInputService::new(GuiInputLimits {
            pointer_leases: if limit == 0 {
                0
            } else {
                1
            },
            path_nodes: 1,
            ..Default::default()
        });
        scene.session = scene.service.open_session().unwrap();
        scene.context = scene
            .service
            .bind_context(&scene.host, &scene.session, scene.root.world())
            .unwrap()
            .context;
        let command = routed(&scene);
        assert!(matches!(
            scene.service.pointer_lease(&command, 9),
            Err(GuiInputError::Capacity)
        ));
        assert_eq!(scene.service.registry.borrow().pointer_leases, 0);
        assert_eq!(scene.service.registry.borrow().pointer_slots.len(), 0);
        assert_eq!(scene.service.registry.borrow().path_nodes, 1);
        assert_eq!(
            terminals(&scene.ledger),
            [GuiDeliveryTerminal::Rejected(GuiInputError::Capacity)]
        );
    }
    let (_, service, _, _, _) = fixture();
    let scene = scene();
    let command = routed(&scene);
    let lease = scene.service.pointer_lease(&command, 1).unwrap();
    assert!(matches!(
        service.pointer_lease(&command, 1),
        Err(GuiInputError::StaleContext)
    ));
    assert_eq!(
        service.cancel_pointer_lease(&lease),
        Err(GuiInputError::SessionClosed)
    );
    command.reject(GuiInputError::Unavailable);
    assert!(matches!(
        scene.service.pointer_lease(&command, 1),
        Err(GuiInputError::Cancelled)
    ));
}
