use super::system_state::GuiSystemState;
use super::tree::node_rows::{GuiNodeDataProperty, GuiNodeDataRow, GuiNodeStyleProperty};
use super::tree::nodes::{
    GuiControlValue, GuiNodeData, GuiNodeHandle, GuiNodeId, GuiNodePatch, GuiNodeStyle,
};
use super::tree::{GuiPartId, GuiPartPatch, GuiPartRow, GuiRoot, GuiThemePartRow};
use crate::systems::state_overlay::{StateOverlaySystem, StateOverlaySystemState};
use crate::systems::surface::Surface;
use crate::systems::{
    System, SystemDependency, SystemDependencyBinding, SystemFactory, SystemId, SystemInitContext,
    SystemInitError,
};
use crate::world::systems::SystemCommitContext;
use crate::{Command, EntityId, ErrorReason, FieldValue, FieldWrite, StateOverlayRef};
use std::collections::BTreeSet;

/// One incremental authoring or control edit of a live GuiRoot component.
#[derive(Clone, Debug, PartialEq)]
pub enum GuiCommand {
    /// Insert one node into the tree at the specified parent and index.
    InsertNode {
        /// Entity owning the GuiRoot.
        entity: EntityId,
        /// Expected root component incarnation.
        root_incarnation: u64,
        /// Node identity to create.
        id: GuiNodeId,
        /// Parent node, or None if root.
        parent: Option<GuiNodeId>,
        /// Child index within parent or root.
        index: u32,
        /// Node kind and authored strings.
        data: GuiNodeData,
        /// Authored kind-specific scalars; presence must match the kind.
        values: GuiNodeDataRow,
        /// Initial style properties.
        style: GuiNodeStyle,
    },
    /// Apply a partial patch to an existing node's data or style.
    UpdateNode {
        /// Fenced node handle.
        handle: GuiNodeHandle,
        /// Data and style patch.
        patch: GuiNodePatch,
    },
    /// Move a node to a new parent or reorder within children.
    MoveNode {
        /// Fenced node handle.
        handle: GuiNodeHandle,
        /// New parent node, or None if root.
        parent: Option<GuiNodeId>,
        /// Child index in new parent.
        index: u32,
    },
    /// Remove a node and its recursive subtree.
    RemoveNode {
        /// Fenced node handle.
        handle: GuiNodeHandle,
    },
    /// Update a committed control value with revision gating.
    SetControlValue {
        /// Fenced node handle.
        handle: GuiNodeHandle,
        /// Expected control revision.
        expected_revision: u32,
        /// New control value to commit.
        value: GuiControlValue,
    },
    /// Create or patch one part of a root theme. The first part of a new
    /// handle defines the theme; referencing nodes re-resolve without edits.
    UpdateTheme {
        /// Entity owning the GuiRoot.
        entity: EntityId,
        /// Expected root component incarnation.
        root_incarnation: u64,
        /// Client-chosen theme handle.
        theme: u32,
        /// Part identity within the theme.
        part: GuiPartId,
        /// Appearance and motion changes.
        patch: GuiPartPatch,
    },
    /// Remove every part of a root theme. Referencing nodes keep the handle
    /// and resolve without a theme until it is defined again.
    RemoveTheme {
        /// Entity owning the GuiRoot.
        entity: EntityId,
        /// Expected root component incarnation.
        root_incarnation: u64,
        /// Theme handle to remove.
        theme: u32,
    },
    /// Patch one node's appearance overrides for one base part; they take
    /// precedence over its theme for every state and variant.
    UpdatePart {
        /// Fenced node handle.
        handle: GuiNodeHandle,
        /// Base part.
        part: crate::systems::surface::GuiPrimitivePart,
        /// Appearance changes; motion is theme-only.
        patch: GuiPartPatch,
    },
}

impl GuiCommand {
    /// The target entity for this command.
    pub fn entity(&self) -> EntityId {
        match self {
            Self::InsertNode {
                entity,
                ..
            }
            | Self::UpdateTheme {
                entity,
                ..
            }
            | Self::RemoveTheme {
                entity,
                ..
            } => *entity,
            Self::UpdateNode {
                handle,
                ..
            }
            | Self::MoveNode {
                handle,
                ..
            }
            | Self::RemoveNode {
                handle,
                ..
            }
            | Self::SetControlValue {
                handle,
                ..
            }
            | Self::UpdatePart {
                handle,
                ..
            } => handle.entity,
        }
    }
}

/// Bounded query to inspect an authoritative GUI root or subtree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuiInspectQuery {
    /// Root entity to inspect.
    pub entity: EntityId,
    /// Starting node, or None to inspect from root.
    pub node_id: Option<GuiNodeId>,
    /// Maximum tree depth to traverse (1..=32).
    pub max_depth: u32,
    /// Maximum number of nodes to return (1..=256).
    pub limit: u32,
}

/// Snapshot of an inspected GUI node.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiInspectedNode {
    /// Node identity.
    pub id: GuiNodeId,
    /// Parent node identity.
    pub parent: Option<GuiNodeId>,
    /// Children identities.
    pub children: Vec<GuiNodeId>,
    /// Node kind and authored strings.
    pub data: GuiNodeData,
    /// Kind-specific scalars, including committed checkbox and slider values.
    pub values: GuiNodeDataRow,
    /// Effective style.
    pub style: GuiNodeStyle,
    /// Committed control value; None for non-control nodes.
    pub control_value: GuiControlValue,
    /// Committed control revision; zero for nodes that were never controls.
    pub control_revision: u32,
}

/// Bounded inspection response containing authoritatively inspected nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiInspectResponse {
    /// Root entity inspected.
    pub root_entity: EntityId,
    /// Root component incarnation.
    pub root_incarnation: u64,
    /// Inspected nodes in tree order.
    pub nodes: Vec<GuiInspectedNode>,
}

/// Ordered GUI edit owner. Every write re-enters ordinary authored lifecycle
/// handling; the tree and committed values of a live root change only here.
#[derive(Default)]
pub struct GuiSystem {
    /// Retained per-world state for this System.
    state: GuiSystemState,
    /// Overlay declarations borrowed to scope withdrawal checks. Absent in
    /// worlds without the overlay system, where no masked contribution can
    /// exist and the staged checks below are vacuous.
    overlay: Option<SystemDependencyBinding<StateOverlaySystem>>,
}

impl GuiSystem {
    /// Stable system identity.
    pub const ID: SystemId = SystemId("ipp.gui");
}

/// Factory for GuiSystem.
#[derive(Default)]
pub struct GuiSystemFactory;

impl SystemFactory for GuiSystemFactory {
    fn id(&self) -> SystemId {
        GuiSystem::ID
    }

    fn dependencies(&self) -> &[SystemDependency] {
        &[
            SystemDependency::After(crate::systems::surface::SurfaceSystem::ID),
            SystemDependency::After(StateOverlaySystem::ID),
        ]
    }

    fn create(
        &self,
        context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(GuiSystem {
            state: GuiSystemState::default(),
            overlay: context
                .dependency::<StateOverlaySystem>(StateOverlaySystem::ID)
                .ok(),
        }))
    }
}

impl System for GuiSystem {
    fn update(&mut self, _context: &mut crate::systems::SystemUpdateContext<'_, '_>) {}

    /// Reject, before any staging, writes that would give a Surface two content
    /// owners or edit a live GUI tree outside GuiCommand, whatever their writer.
    fn before_operation(
        &mut self,
        context: &mut crate::systems::SystemOperationContext<'_>,
    ) -> Result<(), ErrorReason> {
        use crate::ComponentValue as Value;

        let staged = &context.staged;
        let components = &context.world_data.components;
        let has = |entity: EntityId, component| {
            staged
                .entities
                .get(&entity)
                .is_some_and(|record| record.input(component).is_some())
        };
        // Overlays may hide authored items that release would restore, so the
        // effective input, the underlying producer AND every live overlay
        // declaration carrying raw Surface items must be empty.
        // Property-only declarations (dimensions etc.) contribute no
        // restorable content and never block admission.
        let overlay_state = self
            .overlay
            .and_then(|binding| context.dependency(binding))
            .map(|system| &system.state);
        let empty = BTreeSet::new();
        let raw_items =
            |entity| surface_content_live(staged, components, overlay_state, entity, &empty);
        match context.command {
            Command::InsertComponentValue {
                entity,
                value,
            } => {
                let entity = staged.resolve(*entity, context.aliases)?;
                match value {
                    Value::GuiRoot(_) if raw_items(entity) => Err(ErrorReason::InvalidValue),
                    Value::GuiRoot(root) => root.validate_complete(),
                    Value::Surface(surface)
                        if !surface.items().is_empty() && has(entity, Value::GUI_ROOT) =>
                    {
                        Err(ErrorReason::InvalidValue)
                    }
                    _ => Ok(()),
                }
            }
            Command::InsertComponent {
                entity,
                component,
                fields,
            } => {
                let entity = staged.resolve(*entity, context.aliases)?;
                let conflict = match *component {
                    Value::GUI_ROOT => raw_items(entity),
                    Value::SURFACE => {
                        has(entity, Value::GUI_ROOT)
                            && fields
                                .iter()
                                .any(|field| field.offset == Surface::items_field())
                    }
                    _ => false,
                };
                if conflict {
                    return Err(ErrorReason::InvalidValue);
                }
                if *component == Value::GUI_ROOT {
                    // A new incarnation may supply its tree and rows together;
                    // validate the assembled root once rather than per field.
                    validate_inserted_root(fields)?;
                }
                Ok(())
            }
            Command::SetField {
                entity,
                component,
                field,
            } => {
                let entity = staged.resolve(*entity, context.aliases)?;
                if *component == Value::GUI_ROOT
                    && let FieldValue::Dynamic(value) = &field.value
                    && GuiRoot::row_property(field.offset).is_some()
                {
                    GuiRoot::validate_row_value(field.offset, value)?;
                }
                let conflict = match *component {
                    Value::GUI_ROOT => {
                        GuiRoot::command_owned_field(field.offset)
                            && self.state.committing != Some(entity)
                            && has(entity, Value::GUI_ROOT)
                    }
                    Value::SURFACE => {
                        field.offset == Surface::items_field() && has(entity, Value::GUI_ROOT)
                    }
                    _ => false,
                };
                if conflict {
                    Err(ErrorReason::InvalidValue)
                } else {
                    Ok(())
                }
            }
            Command::ReleaseComponentStateOverlay {
                overlay,
                ..
            } => {
                // Withdrawing one declaration restores the layers beneath
                // it. Fail closed before the restore installs masked items
                // beside a live GuiRoot; same-batch aliases resolve within
                // the batch and stay covered by the admission gate above.
                let overlay_state = self
                    .overlay
                    .and_then(|binding| context.dependency(binding))
                    .map(|system| &system.state);
                if let StateOverlayRef::Handle(id) = overlay {
                    let affected = staged
                        .entities
                        .iter()
                        .filter(|(_, record)| {
                            record
                                .layers
                                .get(&crate::ComponentValue::SURFACE)
                                .is_some_and(|layer| layer.inputs.overlay_handles.contains(id))
                        })
                        .map(|(entity, _)| *entity)
                        .collect::<Vec<_>>();
                    let released = BTreeSet::from([*id]);
                    if withdrawal_conflicts(
                        staged,
                        components,
                        overlay_state,
                        affected.into_iter(),
                        &released,
                    ) {
                        return Err(ErrorReason::InvalidValue);
                    }
                }
                Ok(())
            }
            Command::ReleaseStateOverlayOwner {
                owner,
                ..
            } => {
                // Owner cascades resolve inside the overlay lifecycle.
                // Scope precisely through the registry when the owner is a
                // live handle; aliases resolve within the batch and stay
                // covered by admission. Unknown scopes without a registry
                // fail closed only where a violation already lives.
                let overlay = self.overlay.and_then(|binding| context.dependency(binding));
                let overlay_state = overlay.as_ref().map(|system| &system.state);
                let scoped = match (owner, overlay) {
                    (StateOverlayRef::Handle(id), Some(overlays)) => {
                        let owned: BTreeSet<u64> = overlays
                            .state
                            .registry
                            .iter()
                            .filter(|(_, entry)| entry.owner() == Some(*id))
                            .map(|(overlay, _)| overlay)
                            .collect();
                        Some(owned)
                    }
                    _ => None,
                };
                let empty = BTreeSet::new();
                let released = scoped.as_ref().unwrap_or(&empty);
                let affected: Vec<EntityId> = match &scoped {
                    Some(owned) => staged
                        .entities
                        .iter()
                        .filter(|(_, record)| {
                            record
                                .layers
                                .get(&crate::ComponentValue::SURFACE)
                                .is_some_and(|layer| {
                                    layer
                                        .inputs
                                        .overlay_handles
                                        .iter()
                                        .any(|handle| owned.contains(handle))
                                })
                        })
                        .map(|(entity, _)| *entity)
                        .collect(),
                    None => staged.entities.keys().copied().collect(),
                };
                if withdrawal_conflicts(
                    staged,
                    components,
                    overlay_state,
                    affected.into_iter(),
                    released,
                ) {
                    return Err(ErrorReason::InvalidValue);
                }
                Ok(())
            }
            Command::ReleaseEntityOverlayBinding {
                ..
            } => {
                // Binding cascades name no single declaration, so the
                // released set is unknowable here: fail closed only where a
                // live GuiRoot already coexists with restorable Surface
                // content. Clean states pass vacuously.
                let overlay_state = self
                    .overlay
                    .and_then(|binding| context.dependency(binding))
                    .map(|system| &system.state);
                let affected = staged.entities.keys().copied().collect::<Vec<_>>();
                let empty = BTreeSet::new();
                if withdrawal_conflicts(
                    staged,
                    components,
                    overlay_state,
                    affected.into_iter(),
                    &empty,
                ) {
                    return Err(ErrorReason::InvalidValue);
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Report ownership conflicts reached through paths without an operation
    /// gate, such as releasing an overlay that restores raw items.
    fn validate_commit(&self, context: &SystemCommitContext<'_>) -> Result<(), ErrorReason> {
        for &(entity, component) in context.staged.changed.keys() {
            if component != crate::ComponentValue::GUI_ROOT
                && component != crate::ComponentValue::SURFACE
            {
                continue;
            }
            let components = &context.world_data.components;
            let owned = context
                .staged
                .entities
                .get(&entity)
                .is_some_and(|record| record.input(crate::ComponentValue::GUI_ROOT).is_some());
            if owned
                && let Some(crate::ComponentValue::Surface(surface)) =
                    context
                        .staged
                        .input_value(components, entity, crate::ComponentValue::SURFACE)
                && !surface.items().is_empty()
            {
                return Err(ErrorReason::InvalidValue);
            }
        }
        Ok(())
    }

    fn command(
        &mut self,
        context: &mut crate::systems::SystemCommandContext<'_>,
        session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), ErrorReason> {
        let command = command
            .downcast_ref::<GuiCommand>()
            .ok_or(ErrorReason::InvalidValue)?;
        let entity = command.entity();
        let root_incarnation = context
            .world
            .world
            .state
            .entities
            .get(&entity)
            .ok_or(ErrorReason::InvalidEntity)?
            .input(crate::ComponentValue::GUI_ROOT)
            .ok_or(ErrorReason::MissingComponent)?
            .incarnation;
        let root = producer_root(
            &context.world.world.state,
            &context.world.world.components,
            entity,
        )
        .ok_or(ErrorReason::MissingComponent)?;
        let commands = edit_commands(entity, &root, root_incarnation, session, command)?;
        drop(root);

        self.state.committing = Some(entity);
        let result = context.world.apply_authored_commands(Some(self), &commands);
        self.state.committing = None;
        result
    }
}

/// Borrow the producer GuiRoot that authored edits start from: the retained
/// base beneath overlays, or live storage with any hidden overlay originals
/// restored (copying only in that case).
pub(in crate::world::systems::gui) fn producer_root<'a>(
    state: &'a crate::world::WorldEntityState,
    components: &'a crate::components::registry::ComponentStorage,
    entity: EntityId,
) -> Option<std::borrow::Cow<'a, GuiRoot>> {
    let layer = state
        .entities
        .get(&entity)?
        .layers
        .get(&crate::ComponentValue::GUI_ROOT)?;
    layer.inputs.base()?;
    if let Some(value) = layer.inputs.base_value() {
        let crate::ComponentValue::GuiRoot(root) = value else {
            return None;
        };
        return Some(std::borrow::Cow::Borrowed(root));
    }

    let root = components.gui_root(entity.index() as usize)?;
    if layer.inputs.hidden_fields.is_empty() {
        return Some(std::borrow::Cow::Borrowed(root));
    }
    let mut value = crate::ComponentValue::GuiRoot(root.clone());
    layer.inputs.restore_producer(&mut value);
    let crate::ComponentValue::GuiRoot(root) = value else {
        unreachable!("restored GUI root remains a GUI root")
    };
    Some(std::borrow::Cow::Owned(root))
}

/// Ordinary authored writes that apply one GuiCommand to the producer root.
///
/// A node command edits the tree and committed values plus the rows of at
/// most one node, or removes the rows of a removed subtree. The edit runs on
/// an [`GuiRoot::edit_scope`] copy holding only those rows, so validation and
/// the diff against the producer cover the edited node rather than every
/// property; untouched rows were validated when they were written. The writes
/// equal a diff of complete roots, in the same order. Theme and part commands
/// write their rows directly.
pub(super) fn edit_commands(
    entity: EntityId,
    root: &GuiRoot,
    root_incarnation: u64,
    session: u64,
    command: &GuiCommand,
) -> Result<Vec<Command>, ErrorReason> {
    if let Some(writes) = skin_writes(root, root_incarnation, session, command)? {
        let target = crate::EntityRef::Handle(entity);
        return Ok(writes
            .into_iter()
            .map(|field| Command::SetField {
                entity: target,
                component: crate::ComponentValue::GUI_ROOT,
                field,
            })
            .collect());
    }

    match command {
        GuiCommand::InsertNode {
            root_incarnation: expected,
            ..
        } if *expected != root_incarnation => return Err(ErrorReason::InvalidValue),
        GuiCommand::InsertNode {
            ..
        } => {}
        GuiCommand::UpdateTheme {
            ..
        }
        | GuiCommand::RemoveTheme {
            ..
        }
        | GuiCommand::UpdatePart {
            ..
        } => unreachable!("skin commands return their writes above"),
        GuiCommand::UpdateNode {
            handle,
            ..
        }
        | GuiCommand::MoveNode {
            handle,
            ..
        }
        | GuiCommand::RemoveNode {
            handle,
        }
        | GuiCommand::SetControlValue {
            handle,
            ..
        } => validate_handle(root, root_incarnation, session, handle)?,
    }

    let edited = match command {
        GuiCommand::InsertNode {
            id,
            ..
        } => Some(*id),
        GuiCommand::UpdateNode {
            handle,
            ..
        }
        | GuiCommand::SetControlValue {
            handle,
            ..
        } => Some(handle.node_id),
        _ => None,
    };
    let mut next = root.edit_scope(edited)?;
    let mut touched: BTreeSet<GuiNodeId> = edited.into_iter().collect();

    match command {
        GuiCommand::InsertNode {
            id,
            parent,
            index,
            data,
            values,
            style,
            ..
        } => next.insert_node(
            *id,
            *parent,
            *index as usize,
            data.clone(),
            values.clone(),
            style,
        )?,
        GuiCommand::UpdateNode {
            handle,
            patch,
        } => next.update_node(handle.node_id, patch)?,
        GuiCommand::MoveNode {
            handle,
            parent,
            index,
        } => {
            next.nodes_mut()
                .move_node(handle.node_id, *parent, *index as usize)
                .map_err(|_| ErrorReason::InvalidValue)?;
        }
        GuiCommand::RemoveNode {
            handle,
        } => touched.extend(next.remove_node(handle.node_id)?),
        GuiCommand::SetControlValue {
            handle,
            expected_revision,
            value,
        } => next.set_control_value(handle.node_id, *expected_revision, value)?,
        GuiCommand::UpdateTheme {
            ..
        }
        | GuiCommand::RemoveTheme {
            ..
        }
        | GuiCommand::UpdatePart {
            ..
        } => unreachable!("skin commands return their writes above"),
    }

    next.validate_tree()?;
    Ok(scoped_authored_diff(entity, root, &next, &touched))
}

/// Field writes of a theme or part command, or None for node commands. An
/// existing row changes property by property; creating or removing rows
/// replaces the whole table, whose write re-derives the live channels of
/// referencing nodes inside the root.
fn skin_writes(
    root: &GuiRoot,
    root_incarnation: u64,
    session: u64,
    command: &GuiCommand,
) -> Result<Option<Vec<FieldWrite>>, ErrorReason> {
    let fenced = |expected: u64| {
        if expected == root_incarnation {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    };
    Ok(Some(match command {
        GuiCommand::UpdateTheme {
            root_incarnation: expected,
            theme,
            part,
            patch,
            ..
        } => {
            fenced(*expected)?;
            root.theme_part_writes(*theme, *part, patch)?
        }
        GuiCommand::RemoveTheme {
            root_incarnation: expected,
            theme,
            ..
        } => {
            fenced(*expected)?;
            vec![root.theme_removal_write(*theme)?]
        }
        GuiCommand::UpdatePart {
            handle,
            part,
            patch,
        } => {
            validate_handle(root, root_incarnation, session, handle)?;
            root.part_override_writes(handle.node_id, *part, patch)?
        }
        _ => return Ok(None),
    }))
}

/// Validate a GuiRoot assembled from `InsertComponent` fields as a whole.
fn validate_inserted_root(fields: &[FieldWrite]) -> Result<(), ErrorReason> {
    use crate::components::schema::FieldValue as Value;

    let mut root = crate::ComponentValue::GuiRoot(GuiRoot::default());
    for field in fields {
        let value = match &field.value {
            FieldValue::Bytes(bytes) => Value::Bytes(bytes.clone()),
            FieldValue::Rows(bytes) => Value::Rows(bytes.clone()),
            FieldValue::Dynamic(value) => Value::Dynamic(value.clone()),
            FieldValue::Unset => Value::Unset,
            _ => return Err(ErrorReason::InvalidField),
        };
        root.set_field(field.offset, value)
            .map_err(|_| ErrorReason::InvalidField)?;
    }
    let crate::ComponentValue::GuiRoot(root) = root else {
        unreachable!("assembled GUI root remains a GUI root")
    };
    root.validate_complete()
}

/// Whether any live restorable Surface content exists for `entity` outside
/// the withdrawing overlays: staged effective or producer items, or any
/// remaining live overlay declaration carrying the raw items field.
/// Property-only declarations (dimensions etc.) contribute no restorable
/// content. An items declaration counts even when empty, because
/// withdrawing it would restore the masked lower layers beside a live
/// GuiRoot. Unknown handles fail closed.
fn surface_content_live(
    staged: &crate::world::WorldMutationState,
    components: &crate::components::registry::ComponentStorage,
    overlays: Option<&StateOverlaySystemState>,
    entity: EntityId,
    released: &BTreeSet<u64>,
) -> bool {
    use crate::ComponentValue as Value;

    if [
        staged.input_value(components, entity, Value::SURFACE),
        staged.producer_value(components, entity, Value::SURFACE),
    ]
    .into_iter()
    .any(|value| matches!(value, Some(Value::Surface(surface)) if !surface.items().is_empty()))
    {
        return true;
    }
    let handles: Vec<u64> = staged
        .entities
        .get(&entity)
        .and_then(|record| record.layers.get(&Value::SURFACE))
        .map(|layer| {
            layer
                .inputs
                .overlay_handles
                .iter()
                .filter(|handle| !released.contains(handle))
                .copied()
                .collect()
        })
        .unwrap_or_default();
    if handles.is_empty() {
        return false;
    }
    let Some(overlays) = overlays else {
        return true;
    };
    let items = Surface::items_field();
    handles.into_iter().any(|handle| {
        overlays
            .component_overlay_declares(handle, entity, Value::SURFACE, items)
            .unwrap_or(true)
    })
}

/// Whether withdrawing overlays would install conflicting content beside a
/// live GuiRoot. Every affected entity with live GuiRoot input counts any
/// remaining restorable Surface content. Releasing the last content
/// contribution is always safe; property-only overlays and already-clean
/// states pass vacuously.
fn withdrawal_conflicts(
    staged: &crate::world::WorldMutationState,
    components: &crate::components::registry::ComponentStorage,
    overlays: Option<&StateOverlaySystemState>,
    mut affected: impl Iterator<Item = EntityId>,
    released: &BTreeSet<u64>,
) -> bool {
    use crate::ComponentValue as Value;

    affected.any(|entity| {
        staged
            .entities
            .get(&entity)
            .is_some_and(|record| record.input(Value::GUI_ROOT).is_some())
            && surface_content_live(staged, components, overlays, entity, released)
    })
}

/// Overlays may override GUI style, `image_size`, theme part and part row
/// properties, but never supply a GUI tree or whole row table, override
/// committed control values, the slider range, theme references or row keys,
/// create a GuiRoot, or add raw items to a GUI-owned Surface.
///
/// Root ownership is producer-side: reconcilers create the GuiRoot component
/// through `InsertComponent`/`InsertComponentValue`, drive its node tree only
/// through `GuiCommand` edits, and remove the producer with `RemoveComponent`
/// when the root unmounts. Property and state overlays stay `Bound` against
/// that producer and never carry command-owned fields; overlay declarations
/// that would create a root or write those fields are rejected below.
pub(in crate::world) fn validate_overlay_declaration(
    record: &crate::world::WorldEntityRecord,
    component: u16,
    creates: bool,
    mut offsets: impl Iterator<Item = u32>,
) -> Result<(), ErrorReason> {
    let conflict = match component {
        crate::ComponentValue::GUI_ROOT => creates || offsets.any(GuiRoot::command_owned_field),
        crate::ComponentValue::SURFACE => {
            record.input(crate::ComponentValue::GUI_ROOT).is_some()
                && offsets.any(|offset| offset == Surface::items_field())
        }
        _ => false,
    };
    if conflict {
        Err(ErrorReason::InvalidField)
    } else {
        Ok(())
    }
}

/// Revision-gated control commit shared by authored [`GuiCommand`] writes
/// and routed input envelopes. Validates the fenced handle, commits the
/// value into the node's `node_data` row (or text-input record) behind the
/// revision gate and revalidates the tree it changed; staging the result into
/// component storage stays with the caller so each path keeps its own
/// ownership handshake.
pub(super) fn commit_control_value(
    root: &mut GuiRoot,
    root_incarnation: u64,
    session: u64,
    handle: &GuiNodeHandle,
    expected_revision: u32,
    value: &GuiControlValue,
) -> Result<(), ErrorReason> {
    validate_handle(root, root_incarnation, session, handle)?;
    root.set_control_value(handle.node_id, expected_revision, value)?;
    root.validate_tree()
}

/// Re-apply still-active sparse overlay contributions across a producer
/// control commit, returning the refreshed effective value and its hidden
/// producer originals.
///
/// Expected versus actual: refreshed evaluation means the staged effective
/// value is the new producer base with still-active layers re-applied, so
/// when producer and overlay disagree the overlay still wins (producer
/// opacity 1.0 beneath a live Bound 0.25 override stays 0.25). Mutation
/// admission instead installed the raw producer root as the effective value,
/// letting producer style clobber live overrides while their owners stayed
/// attached. The ordinary operation path performs this refresh in
/// `StateOverlayMutationAccess::resolve_layers` (driven by
/// `StateOverlaySystem::after_operation`); input-driven commits stage outside
/// operation hooks, so they re-derive the same contribution here.
///
/// Control commits change only command-owned fields (the tree and the
/// committed `node_data` values), which overlays never carry. The
/// effective-minus-producer delta observed before the commit, per real field,
/// extension value and row property, is therefore exactly the still-active
/// layer contribution; command-owned fields are never carried, so a commit can
/// never clobber its own value.
pub(super) fn resolve_control_effective(
    previous_producer: &crate::ComponentValue,
    previous_effective: &crate::ComponentValue,
    next_producer: crate::ComponentValue,
) -> Result<
    (
        crate::ComponentValue,
        Vec<(u32, crate::components::schema::FieldValue)>,
    ),
    ErrorReason,
> {
    use crate::components::schema::FieldValue as Value;

    let mut winners: Vec<(u32, Value)> = Vec::new();
    for (offset, value) in previous_effective.fields() {
        if GuiRoot::command_owned_field(offset) {
            continue;
        }
        let produced = previous_producer.field(offset).ok();
        if produced.as_ref() != Some(&value) {
            // Mirror layer resolution: an extension value whose descriptor the
            // new producer no longer carries has no contribution to re-apply.
            if crate::components::dynamic_properties::is_dynamic_field(offset)
                && next_producer
                    .dynamic_properties()
                    .and_then(|properties| properties.get_key(offset))
                    .is_none()
            {
                continue;
            }
            winners.push((offset, value));
        }
    }
    if let (
        crate::ComponentValue::GuiRoot(producer),
        crate::ComponentValue::GuiRoot(effective),
        crate::ComponentValue::GuiRoot(next),
    ) = (previous_producer, previous_effective, &next_producer)
    {
        row_winners(producer, effective, next, &mut winners);
    }

    let mut next_effective = next_producer.clone();
    for (offset, value) in &winners {
        next_effective
            .set_field(*offset, value.clone())
            .map_err(|_| ErrorReason::InvalidField)?;
    }
    // Mirror layer resolution: hidden originals are the producer values the
    // winners cover, collected before the winners are applied.
    let hidden = winners
        .iter()
        .map(|(offset, _)| {
            next_producer
                .field(*offset)
                .map(|value| (*offset, value))
                .map_err(|_| ErrorReason::InvalidField)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((next_effective, hidden))
}

/// Row properties whose effective value differs from the producer's, for
/// nodes the new producer still holds. Command-owned data never differs.
fn row_winners(
    producer: &GuiRoot,
    effective: &GuiRoot,
    next: &GuiRoot,
    winners: &mut Vec<(u32, crate::components::schema::FieldValue)>,
) {
    use crate::components::rows::SchemaRow;
    use crate::components::schema::FieldValue as Value;

    let value =
        |property: Option<crate::DynamicValue>| property.map_or(Value::Unset, Value::Dynamic);
    for (slot, row) in effective.node_style().iter() {
        let (Some(produced), true) = (
            producer.node_style().get(slot),
            next.node_style().is_live(slot),
        ) else {
            continue;
        };
        for property in GuiNodeStyleProperty::ALL {
            let current = row.property(property.index()).ok().flatten();
            if current != produced.property(property.index()).ok().flatten()
                && let Some(offset) = GuiRoot::node_style_offset(GuiNodeId(slot), property)
            {
                winners.push((offset, value(current)));
            }
        }
    }
    for (slot, row) in effective.node_data().iter() {
        let (Some(produced), true) = (
            producer.node_data().get(slot),
            next.node_data().is_live(slot),
        ) else {
            continue;
        };
        for property in GuiNodeDataProperty::ALL {
            if property.command_owned() {
                continue;
            }
            let current = row.property(property.index()).ok().flatten();
            if current != produced.property(property.index()).ok().flatten()
                && let Some(offset) = GuiRoot::node_data_offset(GuiNodeId(slot), property)
            {
                winners.push((offset, value(current)));
            }
        }
    }
    skin_row_winners(
        producer.theme_parts(),
        effective.theme_parts(),
        next.theme_parts(),
        GuiThemePartRow::THEME,
        |slot, index| {
            super::tree::GuiPartProperty::from_index(index)
                .and_then(|property| GuiRoot::theme_part_offset(slot, property))
        },
        winners,
    );
    skin_row_winners(
        producer.part_state(),
        effective.part_state(),
        next.part_state(),
        GuiPartRow::NODE,
        GuiRoot::part_row_offset,
        winners,
    );
}

/// Non-key properties of skin rows whose effective value differs from the
/// producer's, for rows the new producer still holds. Keys lead no layout
/// and follow every other property, so `keys` is the first key index.
fn skin_row_winners<R: crate::components::rows::SchemaRow>(
    producer: &crate::components::rows::Rows<R>,
    effective: &crate::components::rows::Rows<R>,
    next: &crate::components::rows::Rows<R>,
    keys: u32,
    offset: impl Fn(u32, u32) -> Option<u32>,
    winners: &mut Vec<(u32, crate::components::schema::FieldValue)>,
) {
    use crate::components::schema::FieldValue as Value;

    for (slot, row) in effective.iter() {
        let (Some(produced), true) = (producer.get(slot), next.is_live(slot)) else {
            continue;
        };
        for index in 0..keys {
            let current = row.property(index).ok().flatten();
            if current != produced.property(index).ok().flatten()
                && let Some(offset) = offset(slot, index)
            {
                winners.push((offset, current.map_or(Value::Unset, Value::Dynamic)));
            }
        }
    }
}

fn validate_handle(
    root: &GuiRoot,
    root_incarnation: u64,
    session: u64,
    handle: &GuiNodeHandle,
) -> Result<(), ErrorReason> {
    if handle.session != session || handle.root_incarnation != root_incarnation {
        return Err(ErrorReason::InvalidValue);
    }
    root.nodes()
        .node(handle.node_id)
        .map(|_| ())
        .ok_or(ErrorReason::InvalidValue)
}

/// Writes turning `previous` into `next` for the tree and the node rows of
/// the touched nodes, which are the only ones `next` holds. The tree comes
/// first: writing it inserts default rows for new nodes, conforms data rows to
/// their kinds and removes the node and part rows of removed nodes. Row
/// properties follow in node order, each written against the row the tree
/// write leaves: style in layout order, data in an order that keeps the
/// slider range valid after every write. A written theme reference
/// re-derives the node's live channels. Every write leaves a valid root.
fn scoped_authored_diff(
    entity: EntityId,
    previous: &GuiRoot,
    next: &GuiRoot,
    touched: &BTreeSet<GuiNodeId>,
) -> Vec<Command> {
    use crate::components::rows::SchemaRow;
    use crate::components::schema::SchemaField;

    let target = crate::EntityRef::Handle(entity);
    let set_field = |offset: u32, value: Option<crate::DynamicValue>| Command::SetField {
        entity: target,
        component: crate::ComponentValue::GUI_ROOT,
        field: FieldWrite {
            offset,
            value: value.map_or(FieldValue::Unset, FieldValue::Dynamic),
        },
    };

    let mut commands = Vec::new();
    if previous.nodes() != next.nodes() {
        let crate::components::schema::FieldValue::Bytes(bytes) = next.nodes().to_value() else {
            unreachable!("GUI node tree is bytes")
        };
        commands.push(Command::SetField {
            entity: target,
            component: crate::ComponentValue::GUI_ROOT,
            field: FieldWrite {
                offset: GuiRoot::nodes_field(),
                value: FieldValue::Bytes(bytes),
            },
        });
    }

    let default_style = super::tree::GuiNodeStyleRow::default();
    let default_data = GuiNodeDataRow::default();
    for &id in touched {
        if let Some(row) = next.style_row(id) {
            let before = previous.style_row(id).unwrap_or(&default_style);
            for property in GuiNodeStyleProperty::ALL {
                let value = row.property(property.index()).ok().flatten();
                if value != before.property(property.index()).ok().flatten()
                    && let Some(offset) = GuiRoot::node_style_offset(id, property)
                {
                    commands.push(set_field(offset, value));
                }
            }
        }
        if let (Some(row), Some(node)) = (next.data_row(id), next.nodes().node(id)) {
            // The tree write conforms the row to the node's kind first.
            let mut before = previous.data_row(id).unwrap_or(&default_data).clone();
            before.conform(&node.data);
            for property in before.write_order(row) {
                let value = row.property(property.index()).ok().flatten();
                if let Some(offset) = GuiRoot::node_data_offset(id, property) {
                    commands.push(set_field(offset, value));
                }
            }
        }
    }

    commands
}

impl crate::WorldContext<'_> {
    /// Queue an incremental GUI edit in ordinary World mutation order.
    pub fn enqueue_gui_command(
        &mut self,
        session: u64,
        command: GuiCommand,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command(GuiSystem::ID, session, command)
    }

    /// Queue a correlated incremental GUI edit in ordinary World mutation order.
    pub fn enqueue_gui_command_with_reply(
        &mut self,
        session: u64,
        request_id: u64,
        command: GuiCommand,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command_with_reply(GuiSystem::ID, session, request_id, command)
    }

    /// Queue ordered GUI edits as one group that stops at the first failure.
    pub fn enqueue_gui_commands_with_reply(
        &mut self,
        session: u64,
        request_id: u64,
        commands: Vec<GuiCommand>,
    ) -> Result<(), ErrorReason> {
        self.enqueue_system_command_batch_with_reply(GuiSystem::ID, session, request_id, commands)
    }

    /// Apply one ordered GUI command buffer under the Host's logical stream gate.
    pub fn apply_gui_command_chunk(
        &mut self,
        session: u64,
        request_id: u64,
        commands: Vec<GuiCommand>,
    ) -> Result<crate::systems::SystemCommandOutcome, ErrorReason> {
        self.apply_system_command_chunk(GuiSystem::ID, session, request_id, commands)
    }

    /// Inspect the current completed effective GuiRoot value.
    pub fn gui_root(&self, entity: EntityId) -> Option<&GuiRoot> {
        self.world.state.entities.get(&entity)?;
        self.world.components.gui_root(entity.index() as usize)
    }

    /// Read-only retained layout view for one root entity, if the layout
    /// pass evaluated it. The view is cloned so no borrow escapes the
    /// call; consumers re-read after each frame instead of retaining
    /// geometry across evaluations.
    pub fn gui_layout_view(&self, entity: EntityId) -> Option<super::GuiEvaluatedView> {
        self.system::<super::GuiLayoutSystem>(super::GuiLayoutSystem::ID)?
            .view(entity)
            .cloned()
    }

    /// Perform a bounded inspection of a GUI root or subtree.
    pub fn inspect_gui(
        &self,
        entity: EntityId,
        node_id: Option<GuiNodeId>,
        max_depth: u32,
        limit: u32,
    ) -> Result<GuiInspectResponse, ErrorReason> {
        let record = self
            .world
            .state
            .entities
            .get(&entity)
            .ok_or(ErrorReason::InvalidEntity)?;
        let gui_root_input = record
            .input(crate::ComponentValue::GUI_ROOT)
            .ok_or(ErrorReason::MissingComponent)?;
        let root_incarnation = gui_root_input.incarnation;

        let root = self.gui_root(entity).ok_or(ErrorReason::MissingComponent)?;
        let start_node_id = match node_id {
            Some(id) => {
                if root.nodes().node(id).is_none() {
                    return Err(ErrorReason::InvalidValue);
                }
                Some(id)
            }
            None => root.nodes().root_node(),
        };

        let depth_limit = max_depth.clamp(1, 32) as usize;
        let count_limit = limit.clamp(1, 256) as usize;

        let mut queue = std::collections::VecDeque::new();
        if let Some(start_id) = start_node_id {
            queue.push_back((start_id, 0usize));
        }

        let mut result_nodes = Vec::new();
        while let Some((curr_id, curr_depth)) = queue.pop_front() {
            if result_nodes.len() >= count_limit {
                break;
            }
            if let Some(node) = root.nodes().node(curr_id) {
                let effective_style = root.style(curr_id).unwrap_or_default();
                let (control_value, control_revision) = root
                    .control_state(curr_id)
                    .map_or((GuiControlValue::None, 0), |state| {
                        (state.value, state.revision)
                    });
                result_nodes.push(GuiInspectedNode {
                    id: node.id,
                    parent: node.parent,
                    children: node.children.clone(),
                    data: node.data.clone(),
                    values: root.data_row(curr_id).cloned().unwrap_or_default(),
                    style: effective_style,
                    control_value,
                    control_revision,
                });

                if curr_depth + 1 < depth_limit {
                    for &child in &node.children {
                        queue.push_back((child, curr_depth + 1));
                    }
                }
            }
        }

        Ok(GuiInspectResponse {
            root_entity: entity,
            root_incarnation,
            nodes: result_nodes,
        })
    }

    /// Validate that a GUI node handle is live and matches its session and root incarnation.
    pub fn validate_gui_node_handle(
        &self,
        handle: &GuiNodeHandle,
        current_session: u64,
    ) -> Result<(), ErrorReason> {
        let root_incarnation = self
            .world
            .state
            .entities
            .get(&handle.entity)
            .ok_or(ErrorReason::InvalidEntity)?
            .input(crate::ComponentValue::GUI_ROOT)
            .ok_or(ErrorReason::MissingComponent)?
            .incarnation;
        let root = self
            .gui_root(handle.entity)
            .ok_or(ErrorReason::MissingComponent)?;
        validate_handle(root, root_incarnation, current_session, handle)
    }
}

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "command_tests.rs"]
mod command_tests;
