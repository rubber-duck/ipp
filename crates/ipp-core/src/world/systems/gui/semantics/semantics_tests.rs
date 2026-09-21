//! Semantic snapshot and action coverage: roles, revisions, diffs and refusals.

use super::*;
use crate::{
    EntityId, GuiContainerKind, GuiControlValue, GuiEvaluatedContent, GuiEvaluatedView,
    GuiInputEffectKind, GuiInspectResponse, GuiInspectedNode, GuiNodeContent, GuiNodeId,
    GuiNodeStyle,
};

fn entity(bits: u64) -> EntityId {
    EntityId::from_bits(bits)
}

fn inspect_fixture() -> GuiInspectResponse {
    GuiInspectResponse {
        root_entity: entity(21),
        root_incarnation: 4,
        nodes: vec![
            GuiInspectedNode {
                id: GuiNodeId(1),
                parent: None,
                children: vec![GuiNodeId(2), GuiNodeId(3), GuiNodeId(4), GuiNodeId(5)],
                content: GuiNodeContent::Container(GuiContainerKind::Column),
                style: GuiNodeStyle::default(),
                control_value: GuiControlValue::None,
                control_revision: 0,
                lifetime: 1,
            },
            GuiInspectedNode {
                id: GuiNodeId(2),
                parent: Some(GuiNodeId(1)),
                children: vec![],
                content: GuiNodeContent::Button {
                    label: "Go".into(),
                },
                style: GuiNodeStyle::default(),
                control_value: GuiControlValue::None,
                control_revision: 0,
                lifetime: 1,
            },
            GuiInspectedNode {
                id: GuiNodeId(3),
                parent: Some(GuiNodeId(1)),
                children: vec![],
                content: GuiNodeContent::Checkbox {
                    checked: false,
                },
                style: GuiNodeStyle::default(),
                control_value: GuiControlValue::Bool(true),
                control_revision: 2,
                lifetime: 1,
            },
            GuiInspectedNode {
                id: GuiNodeId(4),
                parent: Some(GuiNodeId(1)),
                children: vec![],
                content: GuiNodeContent::Slider {
                    value: 0.0,
                    min: 0.0,
                    max: 2.0,
                    step: 0.5,
                },
                style: GuiNodeStyle::default(),
                control_value: GuiControlValue::Scalar(1.5),
                control_revision: 6,
                lifetime: 3,
            },
            GuiInspectedNode {
                id: GuiNodeId(5),
                parent: Some(GuiNodeId(1)),
                children: vec![],
                content: GuiNodeContent::TextInput {
                    text: "seed".into(),
                    placeholder: "name".into(),
                },
                style: GuiNodeStyle::default(),
                control_value: GuiControlValue::Text("ada".into()),
                control_revision: 3,
                lifetime: 1,
            },
        ],
    }
}

fn empty_layout() -> crate::systems::surface::TextLayout {
    crate::systems::surface::TextLayout {
        font_size: 0.1,
        glyphs: Vec::new(),
        lines: Vec::new(),
        grapheme_boundaries: vec![0],
        size: [0.0, 0.0],
    }
}

fn evaluated_node(
    id: u32,
    lifetime: u32,
    rect: [f32; 4],
    content: GuiEvaluatedContent,
) -> crate::GuiEvaluatedNode {
    crate::GuiEvaluatedNode {
        node: GuiNodeId(id),
        lifetime,
        depth: 1,
        rect,
        clip: None,
        content,
        enabled: true,
        visible: true,
        available: true,
        paint_suppressed: false,
        visual_offset: [0.0, 0.0],
        visual_scale: [1.0, 1.0],
        acc_scale: [1.0, 1.0],
        content_extents: None,
        content_origin: [rect[0], rect[1]],
        color: [1.0; 4],
        background: None,
        opacity: 1.0,
    }
}

fn view_fixture(rect_shift: f32) -> GuiEvaluatedView {
    use crate::services::asset_management::AssetKey;
    use crate::services::asset_management::font::FONT_TYPE;
    use crate::systems::surface::SurfaceRenderResource;
    let font = SurfaceRenderResource {
        key: AssetKey {
            slot: 1,
            generation: 1,
        },
        source: crate::services::asset_management::AssetSource {
            kind: FONT_TYPE,
            uri: "font".into(),
            variant: 0,
        },
    };
    GuiEvaluatedView {
        entity: entity(21),
        root_incarnation: 4,
        layout_revision: 8,
        paint_revision: 8,
        evaluation_tick: 12,
        root_bounds: [0.0, 0.0, 10.0, 10.0],
        units_per_metre: 1.0,
        nodes: vec![
            evaluated_node(1, 1, [0.0, 0.0, 10.0, 10.0], GuiEvaluatedContent::Container),
            evaluated_node(
                2,
                1,
                [1.0 + rect_shift, 1.0, 2.0, 1.0],
                GuiEvaluatedContent::Button {
                    layout: empty_layout(),
                    font: font.clone(),
                    font_size: 0.1,
                    label: "Go".into(),
                },
            ),
            evaluated_node(
                3,
                1,
                [1.0, 2.0, 2.0, 1.0],
                GuiEvaluatedContent::Checkbox {
                    checked: true,
                    revision: 2,
                },
            ),
            evaluated_node(
                4,
                3,
                [1.0, 3.0, 4.0, 1.0],
                GuiEvaluatedContent::Slider {
                    value: 1.5,
                    min: 0.0,
                    max: 2.0,
                    step: 0.5,
                    revision: 6,
                },
            ),
            evaluated_node(
                5,
                1,
                [1.0, 4.0, 4.0, 1.0],
                GuiEvaluatedContent::TextInput {
                    layout: empty_layout(),
                    font,
                    font_size: 0.1,
                    text: "ada".into(),
                    revision: 3,
                },
            ),
        ],
        diagnostics: Vec::new(),
        remeasure_count: 0,
        reflow_count: 1,
        available: true,
    }
}

#[test]
fn roles_values_revisions_bounds_exposed() {
    let tree = build_tree(&inspect_fixture(), &view_fixture(0.0)).expect("tree");
    assert_eq!(tree.entity, entity(21));
    assert_eq!(tree.len(), 5);
    let button = tree.node(GuiNodeId(2)).expect("button");
    assert_eq!(button.role, GuiSemanticRole::Button);
    assert_eq!(button.name.as_deref(), Some("Go"));
    assert_eq!(button.actions, vec![GuiSemanticActionKind::Press]);
    assert_eq!(button.bounds, [1.0, 1.0, 2.0, 1.0]);

    let checkbox = tree.node(GuiNodeId(3)).expect("checkbox");
    assert_eq!(checkbox.role, GuiSemanticRole::Checkbox);
    assert_eq!(checkbox.value, GuiControlValue::Bool(true));
    assert_eq!(checkbox.revision, 2);
    assert!(checkbox.actions.contains(&GuiSemanticActionKind::Toggle));

    let slider = tree.node(GuiNodeId(4)).expect("slider");
    assert_eq!(slider.value, GuiControlValue::Scalar(1.5));
    assert_eq!(slider.revision, 6);
    assert_eq!(slider.lifetime, 3);

    let text = tree.node(GuiNodeId(5)).expect("text input");
    assert_eq!(text.value, GuiControlValue::Text("ada".into()));
    assert_eq!(text.name.as_deref(), Some("name"));
    assert!(text.actions.contains(&GuiSemanticActionKind::SetText));
}

#[test]
fn ineligible_nodes_advertise_no_actions_and_refuse_actuation() {
    for state in ["disabled", "hidden", "unavailable"] {
        let mut view = view_fixture(0.0);
        let checkbox = view
            .nodes
            .iter_mut()
            .find(|node| node.node == GuiNodeId(3))
            .unwrap();
        match state {
            "disabled" => checkbox.enabled = false,
            "hidden" => checkbox.visible = false,
            "unavailable" => checkbox.available = false,
            _ => unreachable!(),
        }
        let tree = build_tree(&inspect_fixture(), &view).expect("tree");
        assert!(
            tree.node(GuiNodeId(3)).unwrap().actions.is_empty(),
            "{state}"
        );
        assert_eq!(
            action_command(&tree, GuiNodeId(3), 2, GuiSemanticAction::Toggle),
            Err(GuiSemanticActionError::UnsupportedAction {
                node: GuiNodeId(3),
            }),
            "{state}"
        );
    }
}

#[test]
fn semantic_snapshot_exposes_only_observed_focus() {
    let tree = build_tree(&inspect_fixture(), &view_fixture(0.0)).expect("tree");
    // By construction there is nowhere to store caret, selection,
    // composition, hover, press or scroll state; observed focus is the
    // only transient cursor in the tree.
    assert!(tree.focused.is_none());
    for node in &tree.nodes {
        assert!(node.bounds[2] >= 0.0);
    }
    // Focus moves refresh the tree because focus is observed; hover and
    // scroll cursors have no snapshot fields and never refresh it.
    assert!(effect_refreshes_semantics(
        &GuiInputEffectKind::FocusChanged {
            focus: None
        }
    ));
    assert!(!effect_refreshes_semantics(
        &GuiInputEffectKind::HoverChanged {
            pointer: 2,
            target: None,
            position: [0.0, 0.0],
        }
    ));
    assert!(!effect_refreshes_semantics(
        &GuiInputEffectKind::ScrollChanged {
            entity: entity(21),
            node: GuiNodeId(1),
            offset: [0.0, 1.0],
        }
    ));
    assert!(effect_refreshes_semantics(
        &GuiInputEffectKind::ControlCommitted {
            entity: entity(21),
            root_incarnation: 4,
            node: GuiNodeId(3),
            lifetime: 1,
            value: GuiControlValue::Bool(false),
            revision: 3,
            path: vec![GuiNodeId(3)],
        }
    ));
}

#[test]
fn visual_only_change_preserves_identity() {
    let inspect = inspect_fixture();
    let before = build_tree(&inspect, &view_fixture(0.0)).expect("before");
    let after = build_tree(&inspect, &view_fixture(0.5)).expect("after");
    // Relayout moved the button bounds; identity and revisions hold.
    assert_eq!(
        before.node(GuiNodeId(2)).expect("button").bounds,
        [1.0, 1.0, 2.0, 1.0]
    );
    assert_eq!(
        after.node(GuiNodeId(2)).expect("button").bounds,
        [1.5, 1.0, 2.0, 1.0]
    );
    assert!(changed_nodes(&before, &after).is_empty());
}

#[test]
fn semantic_diff_covers_structure_role_name_state_and_actions() {
    let before = build_tree(&inspect_fixture(), &view_fixture(0.0)).expect("tree");
    let assert_changed = |change: fn(&mut GuiSemanticNode)| {
        let mut after = before.clone();
        change(
            after
                .nodes
                .iter_mut()
                .find(|node| node.id == GuiNodeId(2))
                .unwrap(),
        );
        assert_eq!(changed_nodes(&before, &after), vec![GuiNodeId(2)]);
    };

    assert_changed(|node| node.parent = None);
    assert_changed(|node| node.role = GuiSemanticRole::Text);
    assert_changed(|node| node.name = Some("Renamed".into()));
    assert_changed(|node| node.enabled = false);
    assert_changed(|node| node.visible = false);
    assert_changed(|node| node.available = false);
    assert_changed(|node| node.actions.clear());

    let mut replacement = before.clone();
    replacement.root_incarnation += 1;
    assert_eq!(
        changed_nodes(&before, &replacement),
        before.nodes.iter().map(|node| node.id).collect::<Vec<_>>()
    );
}

#[test]
fn stale_revision_conflicts_instead_of_overwrite() {
    let tree = build_tree(&inspect_fixture(), &view_fixture(0.0)).expect("tree");
    let checkbox = tree.node(GuiNodeId(3)).expect("checkbox");
    assert!(!is_stale_revision(checkbox, 2));
    assert!(is_stale_revision(checkbox, 1));
    assert_eq!(
        action_command(&tree, GuiNodeId(3), 1, GuiSemanticAction::Toggle),
        Err(GuiSemanticActionError::StaleRevision {
            node: GuiNodeId(3),
            expected: 1,
            found: 2,
        })
    );
}

#[test]
fn removed_nodes_invalidate_before_reuse() {
    let before = build_tree(&inspect_fixture(), &view_fixture(0.0)).expect("before");
    let mut inspect = inspect_fixture();
    inspect.nodes.retain(|node| node.id != GuiNodeId(4));
    let mut view = view_fixture(0.0);
    view.nodes.retain(|node| node.node != GuiNodeId(4));
    let after = build_tree(&inspect, &view).expect("after");
    assert!(after.node(GuiNodeId(4)).is_none());
    assert_eq!(changed_nodes(&before, &after), vec![GuiNodeId(4)]);
}

#[test]
fn focus_observed_with_lifetime_fence() {
    let focus = GuiSemanticFocus {
        id: GuiNodeId(5),
        lifetime: 1,
    };
    let tree =
        build_tree_with_focus(&inspect_fixture(), &view_fixture(0.0), Some(focus)).expect("tree");
    assert_eq!(tree.focused, Some(focus));
    // Stale lifetimes never fabricate a target.
    let stale = GuiSemanticFocus {
        id: GuiNodeId(5),
        lifetime: 9,
    };
    let tree =
        build_tree_with_focus(&inspect_fixture(), &view_fixture(0.0), Some(stale)).expect("tree");
    assert!(tree.focused.is_none());
    // Focus-only moves never appear as value changes.
    let before = build_tree(&inspect_fixture(), &view_fixture(0.0)).expect("before");
    assert!(changed_nodes(&before, &tree).is_empty());
}

#[test]
fn actions_dispatch_through_validated_policy() {
    let tree = build_tree(&inspect_fixture(), &view_fixture(0.0)).expect("tree");
    let toggle = action_command(&tree, GuiNodeId(3), 2, GuiSemanticAction::Toggle).expect("toggle");
    assert_eq!(toggle.target.entity, tree.entity);
    assert_eq!(toggle.target.root_incarnation, tree.root_incarnation);
    assert_eq!(toggle.target.node, GuiNodeId(3));
    assert_eq!(toggle.target.lifetime, 1);
    assert_eq!(toggle.expected_revision, 2);
    assert_eq!(toggle.action, GuiSemanticAction::Toggle);

    let text = action_command(
        &tree,
        GuiNodeId(5),
        3,
        GuiSemanticAction::SetText("ada lovelace".into()),
    )
    .expect("set text");
    assert_eq!(text.expected_revision, 3);
    assert_eq!(
        text.action,
        GuiSemanticAction::SetText("ada lovelace".into())
    );
    let scalar = action_command(&tree, GuiNodeId(4), 6, GuiSemanticAction::SetScalar(1.0))
        .expect("set scalar");
    assert_eq!(scalar.action, GuiSemanticAction::SetScalar(1.0));
    let press = action_command(&tree, GuiNodeId(2), 0, GuiSemanticAction::Press).expect("press");
    assert_eq!(press.target.node, GuiNodeId(2));
    assert_eq!(press.action, GuiSemanticAction::Press);
    // Stale revisions conflict instead of overwriting newer input.
    assert_eq!(
        action_command(&tree, GuiNodeId(3), 1, GuiSemanticAction::Toggle),
        Err(GuiSemanticActionError::StaleRevision {
            node: GuiNodeId(3),
            expected: 1,
            found: 2,
        })
    );
    // Unadvertised actions and unknown nodes are refused.
    assert_eq!(
        action_command(&tree, GuiNodeId(3), 2, GuiSemanticAction::Press),
        Err(GuiSemanticActionError::UnsupportedAction {
            node: GuiNodeId(3)
        })
    );
    assert_eq!(
        action_command(&tree, GuiNodeId(9), 0, GuiSemanticAction::Focus),
        Err(GuiSemanticActionError::UnknownNode(GuiNodeId(9)))
    );
}
