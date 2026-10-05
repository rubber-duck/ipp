//! One headless Canvas panel of ordinary GUI entities, driven through real Host frames.
//!
//! Controls give the panel hit targets: raw layout and Canvas entities carry no hits,
//! so geometry tests place buttons where they need to observe evaluated rectangles.
//! Semantic actions are `GuiAction` batch commands; control values are read from their
//! component fields, identities from the completed Canvas publication and focus and
//! pointer feedback from the GUI System queries.

use super::task_scheduler::HostTaskTestDriver;

use ipp_core::components::{GuiButton, GuiCheckbox, GuiLayout, GuiSlider, GuiTextInput};
use ipp_core::systems::canvas::{
    CanvasHit, CanvasHitKind, CanvasPaintEntry, CanvasPart, CanvasPrimitive, CanvasPublication,
};
use ipp_core::systems::gui::GuiSystem;
use ipp_core::systems::gui::layout::GuiEntityLayout;
use ipp_core::systems::gui::local::{
    GuiControlKind, GuiEntityTarget, GuiInteractionFlags, GuiLocalAction,
};
use ipp_core::systems::gui::presentation::{GuiCanvasPublication, GuiControlRecord};
use ipp_core::*;
use std::sync::Arc;

/// A control's value field, read from its stored component.
#[derive(Clone, Debug, PartialEq)]
pub enum ControlValue {
    /// A button has no value.
    None,
    /// A checkbox's `checked`.
    Bool(bool),
    /// A slider's `value`.
    Scalar(f32),
    /// A text input's `text`.
    Text(Arc<str>),
    /// A scroll view's or virtual list's `offset_x` and `offset_y`.
    Scroll([f32; 2]),
    /// A colour control's hue, saturation, value and alpha.
    Color([f32; 4]),
}

/// The role, component id and value of one stored control component.
fn stored_control(value: &ComponentValue) -> Option<(GuiControlKind, u16, ControlValue)> {
    Some(match value {
        ComponentValue::GuiButton(_) => (
            GuiControlKind::Button,
            ComponentValue::GUI_BUTTON,
            ControlValue::None,
        ),
        ComponentValue::GuiCheckbox(value) => (
            GuiControlKind::Checkbox,
            ComponentValue::GUI_CHECKBOX,
            ControlValue::Bool(value.checked),
        ),
        ComponentValue::GuiSlider(value) => (
            GuiControlKind::Slider,
            ComponentValue::GUI_SLIDER,
            ControlValue::Scalar(value.value),
        ),
        ComponentValue::GuiTextInput(value) => (
            GuiControlKind::TextInput,
            ComponentValue::GUI_TEXT_INPUT,
            ControlValue::Text(value.text.clone()),
        ),
        ComponentValue::GuiScrollView(value) => (
            GuiControlKind::ScrollView,
            ComponentValue::GUI_SCROLL_VIEW,
            ControlValue::Scroll([value.offset_x, value.offset_y]),
        ),
        ComponentValue::GuiVirtualList(value) => (
            GuiControlKind::VirtualList,
            ComponentValue::GUI_VIRTUAL_LIST,
            ControlValue::Scroll([value.offset_x, value.offset_y]),
        ),
        ComponentValue::GuiColor(value) => (
            GuiControlKind::Color,
            ComponentValue::GUI_COLOR,
            ControlValue::Color([value.hue, value.saturation, value.value, value.alpha]),
        ),
        _ => return None,
    })
}

/// The control value stored on `entity`, if it carries a control component.
pub fn control_value(world: &WorldContext<'_>, entity: EntityId) -> Option<ControlValue> {
    world
        .inspect(entity)?
        .components
        .iter()
        .find_map(stored_control)
        .map(|(_, _, value)| value)
}

/// A scrolling control's position and evaluated geometry fields, `[x, y]` per quantity.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollFields {
    pub offset: [f32; 2],
    pub viewport: [f32; 2],
    pub content: [f32; 2],
    pub capacity: [f32; 2],
    /// Virtual item count; `None` identifies a ScrollView.
    pub item_count: Option<u32>,
    /// First visible virtual item, zero for a ScrollView.
    pub anchor_index: u32,
    /// Offset within the anchor item.
    pub anchor_offset: f32,
    /// Inclusive first wanted virtual item.
    pub first: u32,
    /// Exclusive last wanted virtual item.
    pub last: u32,
}

/// The scroll fields stored on `entity`'s scroll view or virtual list.
pub fn scroll_fields(world: &WorldContext<'_>, entity: EntityId) -> Option<ScrollFields> {
    world
        .inspect(entity)?
        .components
        .into_iter()
        .find_map(|value| match value {
            ComponentValue::GuiScrollView(view) => Some(ScrollFields {
                offset: [view.offset_x, view.offset_y],
                viewport: [view.viewport_x, view.viewport_y],
                content: [view.content_x, view.content_y],
                capacity: [view.capacity_x, view.capacity_y],
                ..Default::default()
            }),
            ComponentValue::GuiVirtualList(list) => Some(ScrollFields {
                offset: [list.offset_x, list.offset_y],
                viewport: [list.viewport_x, list.viewport_y],
                content: [list.content_x, list.content_y],
                capacity: [list.capacity_x, list.capacity_y],
                item_count: Some(list.item_count),
                anchor_index: list.anchor_index,
                anchor_offset: list.anchor_offset,
                first: list.range_first,
                last: list.range_last,
            }),
            _ => None,
        })
}

/// The control record the latest completed publication of `world` holds for
/// `entity`: its exact target, role, routing value, eligibility and ancestry.
pub fn control_record(
    host: &HostRuntime,
    world: WorldId,
    entity: EntityId,
) -> Option<GuiControlRecord> {
    let publication = host.publication(host.latest_publication(world)?)?;
    let gui = publication
        .chunk(ipp_core::systems::canvas::CanvasSystem::ID)?
        .data::<GuiCanvasPublication>()?;
    gui.views.values().find_map(|view| {
        view.controls
            .iter()
            .find(|control| control.record.target.entity == entity)
            .map(|control| control.record.clone())
    })
}

/// One control read the way a client reads it: identity from the component
/// lifetime, role, value and eligibility from its fields, ancestry from the
/// entity links, and focus and pointer feedback from the GUI System queries.
#[derive(Clone, Debug, PartialEq)]
pub struct ControlRead {
    pub target: GuiEntityTarget,
    pub kind: GuiControlKind,
    /// `GuiBehavior.effective_enabled`.
    pub enabled: bool,
    /// `GuiBehavior.effective_visible`.
    pub visible: bool,
    /// `GuiBehavior.available`.
    pub available: bool,
    /// Root-first core ancestry, including the control.
    pub ancestry: Arc<[EntityId]>,
    pub value: ControlValue,
    pub focused: bool,
    pub interaction: GuiInteractionFlags,
}

/// Read a control of `world` from its stored components, if `entity` is one.
/// No completed publication is needed, so it also reads headless controls.
pub fn read_control(
    host: &mut HostRuntime,
    world: WorldId,
    entity: EntityId,
) -> Option<ControlRead> {
    let context = host.world_mut(world)?;
    let snapshot = context.inspect(entity)?;
    let (kind, component, value) = snapshot.components.iter().find_map(stored_control)?;
    let behavior = snapshot
        .components
        .iter()
        .find_map(|value| match value {
            ComponentValue::GuiBehavior(behavior) => Some(behavior.clone()),
            _ => None,
        })
        .unwrap_or_default();

    let mut ancestry = vec![entity];
    let mut parent = snapshot.link.parent;
    while let Some(id) = parent {
        assert!(!ancestry.contains(&id), "cyclic link above {entity:?}");
        ancestry.push(id);
        parent = context.inspect(id)?.link.parent;
    }
    ancestry.reverse();

    Some(ControlRead {
        target: GuiEntityTarget {
            world: context.world_ref(),
            entity,
            component,
            incarnation: context.component_incarnation(entity, component)?,
        },
        kind,
        enabled: behavior.effective_enabled,
        visible: behavior.effective_visible,
        available: behavior.available,
        ancestry: ancestry.into(),
        value,
        focused: focused(&context, entity),
        interaction: interaction(&context, entity),
    })
}

/// A client's replacement of a control's value: compare-and-set on its value
/// field against the value `read` observed.
pub fn replacement(read: &ControlRead, value: ControlValue) -> Command {
    let (component, offset, expected, value) = match (read.value.clone(), value) {
        (ControlValue::Bool(expected), ControlValue::Bool(value)) => (
            ComponentValue::GUI_CHECKBOX,
            std::mem::offset_of!(GuiCheckbox, checked),
            FieldValue::Bool(expected),
            FieldValue::Bool(value),
        ),
        (ControlValue::Scalar(expected), ControlValue::Scalar(value)) => (
            ComponentValue::GUI_SLIDER,
            std::mem::offset_of!(GuiSlider, value),
            FieldValue::F32(expected),
            FieldValue::F32(value),
        ),
        (ControlValue::Text(expected), ControlValue::Text(value)) => (
            ComponentValue::GUI_TEXT_INPUT,
            std::mem::offset_of!(GuiTextInput, text),
            FieldValue::String(expected),
            FieldValue::String(value),
        ),
        (read, value) => panic!("cannot replace {read:?} with {value:?}"),
    };
    Command::set_field_if(
        EntityRef::Handle(read.target.entity),
        component,
        FieldWrite {
            offset: offset as u32,
            value,
        },
        expected,
    )
}

/// Whether the `GuiFocus` System query names `entity`.
pub fn focused(world: &WorldContext<'_>, entity: EntityId) -> bool {
    !world.gui_focus_page(0, entity.to_bits(), 1).is_empty()
}

/// The union of `entity`'s `GuiPointers` System query records.
pub fn interaction(world: &WorldContext<'_>, entity: EntityId) -> GuiInteractionFlags {
    world
        .gui_pointer_page(0, entity.to_bits(), usize::MAX)
        .into_iter()
        .fold(GuiInteractionFlags::default(), |flags, record| {
            GuiInteractionFlags {
                hovered: flags.hovered || record.state.hovered,
                pressed: flags.pressed || record.state.pressed,
                captured: flags.captured || record.state.captured,
            }
        })
}

/// Tolerance for evaluated logical lengths built from non-binary fractions.
pub const EPSILON: f32 = 1.0e-3;

/// Default panel: 400 x 200 logical units at 100 units per metre.
pub const PANEL: CanvasState = CanvasState {
    extent: [400.0, 200.0],
    units_per_metre: 100.0,
};

/// Batch identities of queued `GuiAction` commands start here.
const ACTION_BATCHES: u64 = 1 << 32;

/// The panel World's Systems: Canvas, GUI and entity layout, with asset
/// dependencies for text, fonts, themes and skins.
pub const PANEL_SYSTEMS: &[systems::SystemId] = &[
    systems::animation::AnimationSystem::ID,
    systems::asset_dependencies::AssetDependencySystem::ID,
    systems::canvas::CanvasSystem::ID,
    GuiSystem::ID,
    systems::gui::GuiLayoutSystem::ID,
];

pub struct GuiPanel {
    pub host: HostRuntime,
    pub world: WorldId,
    pub root: OutputRef,
    /// The canvas's top-level root entity.
    pub root_entity: EntityId,
    next_request: u64,
    /// Batch outcomes of the latest panel frame, in queue order.
    outcomes: Vec<BatchOutcome>,
}

impl GuiPanel {
    /// A panel whose Canvas root also carries `layout`.
    pub fn new(layout: GuiLayout) -> Self {
        Self::with_canvas(PANEL, Some(layout))
    }

    pub fn with_canvas(canvas: CanvasState, layout: Option<GuiLayout>) -> Self {
        Self::with_systems(PANEL_SYSTEMS, canvas, layout)
    }

    /// A panel World selecting exactly `systems`, which must include the panel's.
    pub fn with_systems(
        systems: &[systems::SystemId],
        canvas: CanvasState,
        layout: Option<GuiLayout>,
    ) -> Self {
        let mut host = super::task_scheduler::host();
        let world = host.create_world(Default::default(), systems).unwrap();
        let (root, root_entity) = canvas_root(&mut host, world, canvas, layout);
        Self {
            host,
            world,
            root,
            root_entity,
            next_request: 0,
            outcomes: Vec::new(),
        }
    }

    /// Apply one batch in its own Host frame, which also evaluates it.
    pub fn apply(&mut self, operations: Vec<Command>) -> BatchOutcome {
        apply(&mut self.host, self.world, operations)
    }

    /// Queue one batch so the next panel frame applies and evaluates it; read
    /// its outcome with `take_outcome` after that frame.
    pub fn queue(&mut self, operations: Vec<Command>) {
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue(Batch {
                id: 1,
                operations,
            })
            .unwrap();
    }

    /// Queue one field write for the next panel frame.
    pub fn queue_set(
        &mut self,
        entity: EntityId,
        component: u16,
        offset: usize,
        value: FieldValue,
    ) {
        self.queue(vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component,
            field: FieldWrite {
                offset: offset as u32,
                value,
            },
        }]);
    }

    /// The first batch outcome of the latest panel frame.
    pub fn take_outcome(&mut self) -> BatchOutcome {
        self.outcomes.remove(0)
    }

    /// Create one entity from `values`, placed last under `parent` when given.
    pub fn create(&mut self, parent: Option<EntityId>, values: Vec<ComponentValue>) -> EntityId {
        create(&mut self.host, self.world, parent, values)
    }

    /// A plain layout entity without control behavior or paint.
    pub fn node(&mut self, parent: EntityId, layout: GuiLayout) -> EntityId {
        self.create(Some(parent), vec![ComponentValue::GuiLayout(layout)])
    }

    /// A default button: an eligible hit target sized by `layout`.
    pub fn button(&mut self, parent: EntityId, layout: GuiLayout) -> EntityId {
        self.create(
            Some(parent),
            vec![
                ComponentValue::GuiButton(GuiButton::default()),
                ComponentValue::GuiLayout(layout),
            ],
        )
    }

    pub fn set(
        &mut self,
        entity: EntityId,
        component: u16,
        offset: usize,
        value: FieldValue,
    ) -> BatchOutcome {
        self.apply(vec![Command::SetField {
            entity: EntityRef::Handle(entity),
            component,
            field: FieldWrite {
                offset: offset as u32,
                value,
            },
        }])
    }

    pub fn frame(&mut self) {
        self.frame_for(0.125);
    }

    pub fn frame_for(&mut self, dt: f64) {
        let mut result = self.host.frame_for_test(dt).unwrap();
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
        self.outcomes = result.worlds.remove(&self.world).unwrap().unwrap().outcomes;
        self.outcomes.retain(|outcome| {
            if outcome.batch_id < ACTION_BATCHES {
                return true;
            }
            assert!(outcome.result.is_ok(), "{outcome:?}");
            false
        });
    }

    pub fn output(&self) -> CanvasPublication {
        output(&self.host, self.root)
    }

    pub fn layout(&mut self, entity: EntityId) -> GuiEntityLayout {
        self.host
            .world_mut(self.world)
            .unwrap()
            .gui_entity_layout(entity)
            .unwrap()
    }

    /// The control's record in the latest completed publication.
    pub fn record(&self, entity: EntityId) -> GuiControlRecord {
        control_record(&self.host, self.world, entity).expect("published control")
    }

    /// Read the control the way a client does.
    pub fn snapshot(&mut self, entity: EntityId) -> ControlRead {
        read_control(&mut self.host, self.world, entity).expect("published control")
    }

    /// The control's stored value field.
    pub fn value(&mut self, entity: EntityId) -> ControlValue {
        control_value(&self.host.world_mut(self.world).unwrap(), entity).expect("control")
    }

    /// The scrolling control's stored geometry fields.
    pub fn scroll(&mut self, entity: EntityId) -> ScrollFields {
        scroll_fields(&self.host.world_mut(self.world).unwrap(), entity).expect("scrolling control")
    }

    /// Latest ordinary layout work of this panel's World.
    pub fn work(&mut self) -> ipp_core::systems::gui::layout::GuiEntityLayoutWork {
        self.host
            .world_mut(self.world)
            .unwrap()
            .gui_entity_layout_statistics()
            .unwrap()
            .latest
    }

    /// Queue one `GuiAction` command on the control's published identity;
    /// the next panel frame applies it and checks that it was accepted.
    pub fn act(&mut self, entity: EntityId, action: GuiLocalAction) {
        let target = self.record(entity).target;
        self.next_request += 1;
        self.host
            .world_mut(self.world)
            .unwrap()
            .enqueue(Batch {
                id: ACTION_BATCHES + self.next_request,
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
}

pub fn apply(host: &mut HostRuntime, world: WorldId, operations: Vec<Command>) -> BatchOutcome {
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

/// Create one entity from `values` in `world`, placed last under `parent` when given.
pub fn create(
    host: &mut HostRuntime,
    world: WorldId,
    parent: Option<EntityId>,
    values: Vec<ComponentValue>,
) -> EntityId {
    let mut operations = vec![Command::Create {
        alias: 1,
        metadata: Default::default(),
        adopt: false,
    }];
    operations.extend(
        values
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
    let outcome = apply(host, world, operations);
    outcome.result.unwrap()[0].1
}

/// The canvas of `world` with `canvas` as its state and a top-level root
/// entity, optionally carrying its own layout; returns the canvas output and
/// the root.
pub fn canvas_root(
    host: &mut HostRuntime,
    world: WorldId,
    canvas: CanvasState,
    layout: Option<GuiLayout>,
) -> (OutputRef, EntityId) {
    let world_ref = host.world_ref(world).unwrap();
    let output = super::CanvasTestHost::canvas_output(
        host,
        world_ref,
        canvas.extent,
        canvas.units_per_metre,
    );
    let root = create(
        host,
        world,
        None,
        layout.map(ComponentValue::GuiLayout).into_iter().collect(),
    );
    (output, root)
}

pub fn output(host: &HostRuntime, selection: OutputRef) -> CanvasPublication {
    host.output(
        host.latest_publication(selection.world().id()).unwrap(),
        selection,
    )
    .unwrap()
    .data::<CanvasPublication>()
    .unwrap()
    .clone()
}

/// `[min_x, min_y, max_x, max_y]` of the rectangle at `[x, y]` with `size`.
pub fn rect(x: f32, y: f32, width: f32, height: f32) -> [f32; 4] {
    [x, y, x + width, y + height]
}

pub fn assert_near(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    assert!(
        actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| (actual - expected).abs() <= EPSILON),
        "{actual:?} != {expected:?}"
    );
}

/// The control's own entity hit.
pub fn control_hit(publication: &CanvasPublication, entity: EntityId) -> &CanvasHit {
    publication
        .hits
        .iter()
        .find(|hit| hit.target.entity == entity && hit.kind == CanvasHitKind::Entity)
        .unwrap_or_else(|| panic!("no hit for {entity:?}"))
}

pub fn bounds(publication: &CanvasPublication, entity: EntityId) -> [f32; 4] {
    control_hit(publication, entity).bounds
}

/// Reverse-painter target of a Canvas-local point; the highest-priority, latest
/// painted eligible hit wins.
pub fn hit_at(publication: &CanvasPublication, point: [f32; 2]) -> Option<EntityId> {
    publication
        .hits
        .iter()
        .enumerate()
        .filter(|(_, hit)| hit.contains(point))
        .max_by_key(|(index, hit)| (hit.priority, hit.paint_order, *index))
        .map(|(_, hit)| hit.target.entity)
}

pub fn primitive(entry: &CanvasPaintEntry) -> &CanvasPrimitive {
    let CanvasPaintEntry::Primitive {
        primitive,
        ..
    } = entry
    else {
        panic!("expected a primitive, found {entry:?}")
    };
    primitive
}

/// Painted primitives of one entity and part, in painter order.
pub fn parts(
    publication: &CanvasPublication,
    entity: EntityId,
    part: CanvasPart,
) -> Vec<CanvasPrimitive> {
    publication
        .entries
        .iter()
        .filter_map(|entry| match entry.as_ref() {
            CanvasPaintEntry::Primitive {
                primitive,
                ..
            } if primitive.style().identity.target.entity == entity
                && primitive.style().identity.part == part =>
            {
                Some(primitive.clone())
            }
            _ => None,
        })
        .collect()
}
