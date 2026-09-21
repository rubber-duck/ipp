use super::{Surface, SurfaceItemContent, SurfaceItemId, SurfaceItemPatch, SurfaceItemStyle};
use crate::components::schema::SchemaComponent;
use crate::systems::{System, SystemFactory, SystemId, SystemInitContext, SystemInitError};

/// One incremental edit of a live Surface component.
#[derive(Clone, Debug, PartialEq)]
#[allow(missing_docs)]
pub enum SurfaceCommand {
    Insert {
        entity: crate::EntityId,
        id: SurfaceItemId,
        index: u32,
        content: SurfaceItemContent,
        style: SurfaceItemStyle,
    },
    Update {
        entity: crate::EntityId,
        id: SurfaceItemId,
        patch: SurfaceItemPatch,
    },
    Remove {
        entity: crate::EntityId,
        id: SurfaceItemId,
    },
    Move {
        entity: crate::EntityId,
        id: SurfaceItemId,
        index: u32,
    },
}

/// Ordered Surface edit owner. All writes re-enter ordinary authored lifecycle handling.
#[derive(Default)]
pub struct SurfaceSystem;

impl SurfaceSystem {
    /// Stable composition and generic command-routing identity.
    pub const ID: SystemId = SystemId("ipp.surface");
}

/// Reusable factory retaining no World state.
#[derive(Default)]
pub struct SurfaceSystemFactory;

impl SystemFactory for SurfaceSystemFactory {
    fn id(&self) -> SystemId {
        SurfaceSystem::ID
    }

    fn dependencies(&self) -> &[crate::systems::SystemDependency] {
        &[]
    }

    fn create(
        &self,
        _context: &mut SystemInitContext<'_>,
    ) -> Result<Box<dyn System>, SystemInitError> {
        Ok(Box::new(SurfaceSystem))
    }
}

impl System for SurfaceSystem {
    fn update(&mut self, _context: &mut crate::systems::SystemUpdateContext<'_, '_>) {}

    fn command(
        &mut self,
        context: &mut crate::systems::SystemCommandContext<'_>,
        _session: u64,
        command: &dyn std::any::Any,
    ) -> Result<(), crate::ErrorReason> {
        let command = command
            .downcast_ref::<SurfaceCommand>()
            .ok_or(crate::ErrorReason::InvalidValue)?;
        let entity = match command {
            SurfaceCommand::Insert {
                entity,
                ..
            }
            | SurfaceCommand::Update {
                entity,
                ..
            }
            | SurfaceCommand::Remove {
                entity,
                ..
            }
            | SurfaceCommand::Move {
                entity,
                ..
            } => *entity,
        };
        #[cfg(feature = "gui")]
        if context
            .world
            .world
            .components
            .gui_root(entity.index() as usize)
            .is_some()
        {
            return Err(crate::ErrorReason::InvalidValue);
        }
        if let SurfaceCommand::Update {
            id,
            patch,
            ..
        } = command
            && patch.asset.is_none()
            && context
                .world
                .can_apply_authored_in_place(entity, crate::ComponentValue::SURFACE)
        {
            let surface = context
                .world
                .world
                .components
                .surface(entity.index() as usize)
                .ok_or(crate::ErrorReason::MissingComponent)?;
            surface.validate_item_patch(*id, patch)?;
            let mut fields = Vec::with_capacity(6);
            if patch.content.is_some() {
                fields.push(Surface::items_field());
            }
            for (changed, suffix) in [
                (patch.position.is_some(), "position"),
                (patch.scale.is_some(), "scale"),
                (patch.color.is_some(), "color"),
                (patch.opacity.is_some(), "opacity"),
                (patch.font_size.is_some(), "font_size"),
            ] {
                if changed {
                    let name = Surface::property_name(*id, suffix).unwrap();
                    fields.push(
                        surface
                            .properties
                            .key(&name)
                            .ok_or(crate::ErrorReason::InvalidField)?,
                    );
                }
            }
            let id = *id;
            let patch = patch.clone();
            return context.world.apply_authored_in_place(
                self,
                entity,
                crate::ComponentValue::SURFACE,
                fields,
                move |storage| {
                    storage
                        .surface_mut(entity.index() as usize)
                        .ok_or(crate::ErrorReason::MissingComponent)?
                        .update_item(id, patch)
                },
            );
        }
        let crate::ComponentValue::Surface(mut surface) = context
            .world
            .world
            .state
            .producer_value(
                &context.world.world.components,
                entity,
                crate::ComponentValue::SURFACE,
            )
            .ok_or(crate::ErrorReason::MissingComponent)?
        else {
            unreachable!("surface component")
        };
        let previous = surface.clone();
        match command.clone() {
            SurfaceCommand::Insert {
                id,
                index,
                content,
                style,
                ..
            } => {
                if id.0 != surface.next_item_id() {
                    return Err(crate::ErrorReason::InvalidValue);
                }
                let actual = surface.insert_item(index as usize, content, style)?;
                debug_assert_eq!(actual, id);
            }
            SurfaceCommand::Update {
                id,
                patch,
                ..
            } => surface.update_item(id, patch)?,
            SurfaceCommand::Remove {
                id,
                ..
            } => {
                surface.remove_item(id)?;
            }
            SurfaceCommand::Move {
                id,
                index,
                ..
            } => surface.move_item(id, index as usize)?,
        }
        surface.validate_complete()?;
        let commands = authored_diff(entity, &previous, &surface);
        context.world.apply_authored_commands(Some(self), &commands)
    }
}

fn authored_diff(
    entity: crate::EntityId,
    previous: &Surface,
    next: &Surface,
) -> Vec<crate::Command> {
    let mut commands = Vec::new();
    for (name, descriptor) in previous.properties.descriptors() {
        if !next.properties.descriptors().contains_key(name) {
            commands.push(crate::Command::RemoveDynamicProperty {
                entity: crate::EntityRef::Handle(entity),
                component: crate::ComponentValue::SURFACE,
                name: name.clone(),
            });
        } else if next.properties.get(name) != previous.properties.get_key(descriptor.key) {
            commands.push(set_property(
                entity,
                name,
                next.properties.get(name).unwrap(),
            ));
        }
    }
    for name in next.properties.descriptors().keys() {
        if !previous.properties.descriptors().contains_key(name) {
            commands.push(set_property(
                entity,
                name,
                next.properties.get(name).unwrap(),
            ));
        }
    }
    for ((offset, previous), (_, next)) in previous.fields().into_iter().zip(next.fields()) {
        if crate::components::dynamic_properties::is_dynamic_field(offset) || previous == next {
            continue;
        }
        commands.push(crate::Command::SetField {
            entity: crate::EntityRef::Handle(entity),
            component: crate::ComponentValue::SURFACE,
            field: crate::FieldWrite {
                offset,
                value: schema_value(next),
            },
        });
    }
    commands
}

fn set_property(entity: crate::EntityId, name: &str, value: crate::DynamicValue) -> crate::Command {
    crate::Command::SetDynamicProperty {
        entity: crate::EntityRef::Handle(entity),
        component: crate::ComponentValue::SURFACE,
        name: name.into(),
        value,
    }
}

fn schema_value(value: crate::components::schema::FieldValue) -> crate::FieldValue {
    match value {
        crate::components::schema::FieldValue::F32(value) => crate::FieldValue::F32(value),
        crate::components::schema::FieldValue::U32(value) => crate::FieldValue::U32(value),
        crate::components::schema::FieldValue::Bytes(value) => crate::FieldValue::Bytes(value),
        _ => unreachable!("surface structural field type"),
    }
}

impl crate::WorldContext<'_> {
    /// Queue an incremental Surface edit in ordinary World mutation order.
    pub fn enqueue_surface_command(
        &mut self,
        session: u64,
        command: SurfaceCommand,
    ) -> Result<(), crate::ErrorReason> {
        self.enqueue_system_command(SurfaceSystem::ID, session, command)
    }

    /// Queue a correlated incremental Surface edit in ordinary World mutation order.
    pub fn enqueue_surface_command_with_reply(
        &mut self,
        session: u64,
        request_id: u64,
        command: SurfaceCommand,
    ) -> Result<(), crate::ErrorReason> {
        self.enqueue_system_command_with_reply(SurfaceSystem::ID, session, request_id, command)
    }

    /// Inspect the current completed effective Surface value.
    pub fn surface(&self, entity: crate::EntityId) -> Option<&Surface> {
        self.world.state.entities.get(&entity)?;
        self.world.components.surface(entity.index() as usize)
    }

    /// Conservative centred local rectangle for culling or plane interaction.
    pub fn surface_bounding_geometry(
        &self,
        entity: crate::EntityId,
    ) -> Option<crate::systems::geometry::GeometryShape> {
        Some(self.surface(entity)?.local_bounding_geometry())
    }

    /// Map an entity-local XY plane hit to Surface content coordinates if within bounds.
    pub fn surface_plane_hit(
        &self,
        entity: crate::EntityId,
        entity_x: f32,
        entity_y: f32,
    ) -> Option<[f32; 2]> {
        self.surface(entity)?
            .plane_hit_to_content(entity_x, entity_y)
    }

    /// Map a Surface content point ([0, width] x [0, height], +X right, +Y down) to centred entity-local coordinates.
    pub fn surface_content_to_entity_local(
        &self,
        entity: crate::EntityId,
        x: f32,
        y: f32,
    ) -> Option<[f32; 3]> {
        Some(self.surface(entity)?.content_to_entity_local(x, y))
    }
}
