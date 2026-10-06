//! World request decoding: framing, tags, field domains, counts and byte limits,
//! without World or Host state.

use super::view_queries::{CameraNavigateRequest, CameraProjectQuery, GeometryPickQuery};
use super::{BatchPage, InspectionQuery, Request, RequestBody};
use crate::MAX_MESSAGE_BYTES;
use crate::codec::{ProtocolError, Reader};
use crate::contract::wire_manifest::*;
use ipp_core::{Command, EntityId, EntityMetadata, EntityRef, FieldValue, FieldWrite};

impl Reader<'_> {
    pub(super) fn render_state_patch(
        &mut self,
    ) -> Result<ipp_core::RenderStatePatch, ProtocolError> {
        let mask = self.u16()?;
        if mask & !7 != 0 {
            return Err(ProtocolError::Malformed("render state mask"));
        }
        Ok(ipp_core::RenderStatePatch {
            show_all_debug_geometries: if mask & 1 != 0 {
                Some(self.boolean()?)
            } else {
                None
            },
            debug_geometry_color: if mask & 2 != 0 {
                Some([self.f32()?, self.f32()?, self.f32()?])
            } else {
                None
            },
            ambient_light: if mask & 4 != 0 {
                Some([self.f32()?, self.f32()?, self.f32()?])
            } else {
                None
            },
        })
    }

    /// Finite extent and density; positivity is the Canvas System's to validate.
    pub(crate) fn canvas_state(&mut self) -> Result<ipp_core::CanvasState, ProtocolError> {
        Ok(ipp_core::CanvasState {
            extent: [self.f32()?, self.f32()?],
            units_per_metre: self.f32()?,
        })
    }

    pub(super) fn gui_preferences_update(
        &mut self,
    ) -> Result<ipp_core::systems::gui::GuiPreferencesUpdate, ProtocolError> {
        let mask = self.u16()?;
        if mask & !1 != 0 {
            return Err(ProtocolError::Malformed("GUI preferences mask"));
        }
        Ok(ipp_core::systems::gui::GuiPreferencesUpdate {
            reduced_motion: if mask & 1 != 0 {
                Some(self.boolean()?)
            } else {
                None
            },
        })
    }

    pub(super) fn canvas_state_update(
        &mut self,
    ) -> Result<ipp_core::CanvasStateUpdate, ProtocolError> {
        let mask = self.u16()?;
        if mask & !3 != 0 {
            return Err(ProtocolError::Malformed("canvas state mask"));
        }
        Ok(ipp_core::CanvasStateUpdate {
            extent: if mask & 1 != 0 {
                Some([self.f32()?, self.f32()?])
            } else {
                None
            },
            units_per_metre: if mask & 2 != 0 {
                Some(self.f32()?)
            } else {
                None
            },
        })
    }

    pub(super) fn metadata(&mut self) -> Result<EntityMetadata, ProtocolError> {
        let symbolic_id = match self.u8()? {
            OPTION_NONE => None,
            OPTION_SOME => Some(self.string()?),
            _ => return Err(ProtocolError::Malformed("option tag")),
        };
        let n = self.count(crate::MAX_METADATA_CLASSES)?;
        let mut classes = Vec::with_capacity(n);
        for _ in 0..n {
            classes.push(self.string()?);
        }
        Ok(EntityMetadata {
            symbolic_id,
            classes,
        })
    }

    pub(super) fn entity(&mut self) -> Result<EntityRef, ProtocolError> {
        match self.u8()? {
            REF_HANDLE => Ok(EntityRef::Handle(EntityId::from_bits(self.u64()?))),
            REF_ALIAS => Ok(EntityRef::Alias(self.u32()?)),
            REF_SYMBOL => Ok(EntityRef::Symbol(self.text()?)),
            tag => Err(ProtocolError::Unsupported(tag)),
        }
    }

    pub(super) fn field(&mut self) -> Result<FieldWrite, ProtocolError> {
        let offset = self.u32()?;
        if ipp_core::components::dynamic_properties::is_dynamic_field(offset) {
            return Err(ProtocolError::Malformed(
                "named property requires named addressing",
            ));
        }
        Ok(FieldWrite {
            offset,
            value: self.field_value()?,
        })
    }

    /// Decode one tagged field value without its offset.
    pub(super) fn field_value(&mut self) -> Result<FieldValue, ProtocolError> {
        Ok(match self.u8()? {
            VALUE_WORLD => {
                if self.boolean()? {
                    let reference = self.world_reference()?;
                    FieldValue::UnresolvedWorld(ipp_core::WorldReferenceToken::untrusted(
                        reference.id,
                        reference.incarnation,
                    ))
                } else {
                    FieldValue::World(None)
                }
            }
            VALUE_OUTPUT => {
                if self.boolean()? {
                    let reference = self.output_reference()?;
                    let target = reference.target.core();
                    FieldValue::UnresolvedOutput(ipp_core::OutputReferenceToken::untrusted(
                        ipp_core::WorldReferenceToken::untrusted(
                            reference.world.id,
                            reference.world.incarnation,
                        ),
                        target,
                    ))
                } else {
                    FieldValue::Output(None)
                }
            }
            VALUE_BOOL => FieldValue::Bool(self.boolean()?),
            VALUE_F32 => FieldValue::F32(self.f32()?),
            VALUE_ENTITY => FieldValue::Entity(self.entity()?),
            VALUE_U32 => FieldValue::U32(self.u32()?),
            VALUE_U64 => FieldValue::U64(self.u64()?),
            VALUE_STRING => FieldValue::String(self.text()?),
            VALUE_BYTES => FieldValue::Bytes(self.bytes()?),
            VALUE_DYNAMIC => FieldValue::Dynamic(
                ipp_core::DynamicValue::decode(&self.bytes()?)
                    .map_err(|_| ProtocolError::Malformed("dynamic value"))?,
            ),
            VALUE_ROWS => FieldValue::Rows(self.bytes_bounded(crate::MAX_MESSAGE_BYTES)?),
            VALUE_UNSET => FieldValue::Unset,
            tag => return Err(ProtocolError::Unsupported(tag)),
        })
    }

    fn placement(&mut self) -> Result<ipp_core::EntityPlacementRef, ProtocolError> {
        let parent = match self.u8()? {
            OPTION_NONE => None,
            OPTION_SOME => Some(self.entity()?),
            _ => return Err(ProtocolError::Malformed("placement parent")),
        };
        let before = match self.u8()? {
            OPTION_NONE => None,
            OPTION_SOME => Some(self.entity()?),
            _ => return Err(ProtocolError::Malformed("placement sibling")),
        };
        Ok(ipp_core::EntityPlacementRef {
            parent,
            before,
        })
    }

    /// Decode the commands of one batch page into `operations` through the end of
    /// the message.
    fn batch_page(
        &mut self,
        message_bytes: usize,
        operations: &mut Vec<Command>,
    ) -> Result<(), ProtocolError> {
        if message_bytes > crate::COMMAND_PAGE_BYTES {
            return Err(ProtocolError::Limit("command page bytes"));
        }
        let n = self.count(crate::COMMAND_PAGE_COMMANDS)?;
        operations.clear();
        // Geometric growth could retain more than the wire limit for later batches.
        operations.reserve_exact(n);
        for _ in 0..n {
            operations.push(self.command()?);
        }
        if self.at != self.bytes.len() {
            return Err(ProtocolError::Malformed("trailing bytes"));
        }
        Ok(())
    }

    pub(super) fn command(&mut self) -> Result<Command, ProtocolError> {
        Ok(match self.u8()? {
            COMMAND_CREATE => Command::Create {
                alias: self.u32()?,
                metadata: self.metadata()?,
                adopt: self.boolean()?,
            },
            COMMAND_DELETE => Command::Delete {
                entity: self.entity()?,
            },
            COMMAND_PLACE_ENTITY => Command::PlaceEntity {
                entity: self.entity()?,
                placement: self.placement()?,
            },
            COMMAND_DELETE_SUBTREE => Command::DeleteSubtree {
                root: self.entity()?,
            },
            COMMAND_DETACH_ATTACHMENT_RECEIPT => Command::DetachWorldAttachmentReceipt {
                receipt: self.u64()?,
            },
            COMMAND_METADATA => Command::SetMetadata {
                entity: self.entity()?,
                metadata: self.metadata()?,
            },
            COMMAND_INSERT => {
                let entity = self.entity()?;
                let component = self.u16()?;
                let n = self.count(crate::MAX_INSERT_FIELDS)?;
                let mut fields = Vec::with_capacity(n);
                for _ in 0..n {
                    fields.push(self.field()?);
                }
                Command::InsertComponent {
                    entity,
                    component,
                    fields,
                    adopt: self.boolean()?,
                }
            }
            COMMAND_SET_DYNAMIC_PROPERTY => Command::SetDynamicProperty {
                entity: self.entity()?,
                component: self.u16()?,
                name: self.string()?,
                value: ipp_core::DynamicValue::decode(&self.bytes()?)
                    .map_err(|_| ProtocolError::Malformed("dynamic value"))?,
            },
            COMMAND_REMOVE_DYNAMIC_PROPERTY => Command::RemoveDynamicProperty {
                entity: self.entity()?,
                component: self.u16()?,
                name: self.string()?,
            },
            COMMAND_SET => Command::SetField {
                entity: self.entity()?,
                component: self.u16()?,
                field: self.field()?,
            },
            COMMAND_SET_FIELD_IF => {
                let entity = self.entity()?;
                let component = self.u16()?;
                let field = self.field()?;
                let expected = self.field_value()?;
                Command::set_field_if(entity, component, field, expected)
            }
            COMMAND_REMOVE => Command::RemoveComponent {
                entity: self.entity()?,
                component: self.u16()?,
            },
            COMMAND_GUI_ACTION => Command::GuiAction {
                target: ipp_core::GuiActionTarget {
                    entity: self.entity()?,
                    component: self.u16()?,
                    incarnation: self.u64()?,
                },
                action: self.gui_action()?,
            },
            tag => return Err(ProtocolError::Unsupported(tag)),
        })
    }
}

/// Decode one bounded complete message into owned core commands.
/// Call only after `accept_hello` succeeds for this connection.
pub fn decode_request(bytes: &[u8], expected_session: u64) -> Result<Request, ProtocolError> {
    decode_request_with_buffer(bytes, expected_session, &mut Vec::new())
}

/// Decode with caller-owned batch capacity. Successful batches take the buffer;
/// other requests leave it available. On failure, callers must clear/recycle any
/// retained operations; a fully decoded rejected message may already have taken it.
pub fn decode_request_with_buffer(
    bytes: &[u8],
    expected_session: u64,
    operations: &mut Vec<Command>,
) -> Result<Request, ProtocolError> {
    decode_world_request(bytes, Some(expected_session), operations).map_err(|error| match error {
        RequestDecodeError::Request(error) => error,
        RequestDecodeError::BatchPage(page) => page.error,
    })
}

/// Whether a World request message is a batch page, judged by its request tag
/// without decoding it.
pub fn is_batch_page(bytes: &[u8]) -> bool {
    bytes.get(16) == Some(&REQUEST_SUBMIT_BATCH)
}

/// A batch page whose identifying header decoded but whose content did not.
///
/// The failure belongs to the page's batch, not to its connection: the Host
/// fails that batch and answers its final page with this error.
#[derive(Clone, Debug, PartialEq)]
pub struct RejectedBatchPage {
    /// Session named by the page.
    pub session: u64,
    /// Correlated identity, nonzero exactly on the final page.
    pub request_id: u64,
    /// Client-assigned batch identity.
    pub batch_id: u32,
    /// Whether this page completes the batch.
    pub last: bool,
    /// First decoding error of the page.
    pub error: ProtocolError,
}

/// Why a World request could not be decoded.
#[derive(Clone, Debug, PartialEq)]
pub enum RequestDecodeError {
    /// The message cannot be attributed to a batch page; its connection fails.
    Request(ProtocolError),
    /// An identified batch page is malformed; only its batch fails.
    BatchPage(RejectedBatchPage),
}

impl From<ProtocolError> for RequestDecodeError {
    fn from(error: ProtocolError) -> Self {
        Self::Request(error)
    }
}

/// Decode a World request without World or Host state, so a transport may run it
/// as soon as a message arrives. `None` accepts any nonzero session; the caller
/// then checks that the session belongs to the receiving connection.
///
/// Performs every check that needs no World state: framing, tags, field domains,
/// counts and byte limits. Entity references, aliases and schema checks stay with
/// each command's mutation boundary.
pub fn decode_world_request(
    bytes: &[u8],
    expected_session: Option<u64>,
    operations: &mut Vec<Command>,
) -> Result<Request, RequestDecodeError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::Limit("message").into());
    }

    let mut r = Reader {
        bytes,
        at: 0,
    };
    let session = r.u64()?;
    if session == 0 || expected_session.is_some_and(|expected| session != expected) {
        return Err(ProtocolError::SessionMismatch.into());
    }

    let request_id = r.u64()?;

    let body = match r.u8()? {
        REQUEST_GUI_OBSERVATION => RequestBody::GuiObservation(r.gui_observation_request()?),
        REQUEST_ATTACHMENT_RECEIPT => RequestBody::AttachmentReceipt {
            receipt: r.u64()?,
            release: match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err(ProtocolError::Malformed("receipt release flag").into()),
            },
        },
        REQUEST_SUBMIT_BATCH => {
            let batch_id = r.u32()?;
            let last = match r.u8()? {
                0 => false,
                1 => true,
                _ => return Err(ProtocolError::Malformed("batch page completion flag").into()),
            };
            // A batch is answered once, on its final page, so earlier pages are
            // uncorrelated and never leave correlation state.
            if last == (request_id == 0) {
                return Err(ProtocolError::Malformed("reserved request identity").into());
            }
            return r
                .batch_page(bytes.len(), operations)
                .map(|()| Request {
                    session,
                    request_id,
                    body: RequestBody::SubmitBatch(BatchPage {
                        batch_id,
                        last,
                        operations: std::mem::take(operations),
                    }),
                })
                .map_err(|error| {
                    RequestDecodeError::BatchPage(RejectedBatchPage {
                        session,
                        request_id,
                        batch_id,
                        last,
                        error,
                    })
                });
        }
        REQUEST_INSPECT => {
            let collection = r.u8()?;
            let after = r.u64()?;
            let target = r.u64()?;
            let limit = r.u16()?;
            let max_depth = r.u16()?;
            let gui_collection = matches!(
                collection,
                INSPECT_GUI_FOCUS
                    | INSPECT_GUI_POINTERS
                    | INSPECT_GUI_ACTIVE_ITEMS
                    | INSPECT_GUI_PREFERENCES
            );
            let canvas_collection = collection == INSPECT_CANVAS;
            if !(collection <= INSPECT_ENTITY_TREE || gui_collection || canvas_collection)
                || limit == 0
                || usize::from(limit) > crate::INSPECTION_PAGE_RECORDS
                || (collection != 5 && (max_depth != 0 || (target != 0 && after != 0)))
                || (collection == 5 && max_depth > crate::MAX_ENTITY_TREE_DEPTH)
            {
                return Err(ProtocolError::Malformed("inspection query").into());
            }
            RequestBody::Inspect(InspectionQuery {
                collection,
                after,
                target,
                limit,
                max_depth,
            })
        }
        REQUEST_LIFECYCLE_WATCH => RequestBody::LifecycleWatch(r.lifecycle_watch_request()?),
        REQUEST_LIFECYCLE_DIAGNOSTICS => {
            RequestBody::LifecycleDiagnostics(r.lifecycle_diagnostic_query()?)
        }
        REQUEST_LIFECYCLE_SUBSCRIBE => RequestBody::LifecycleSubscription(
            ipp_core::systems::lifecycle_publisher::LifecyclePublisherCommand::Subscribe {
                subscription: r.lifecycle_subscription_id()?,
                filter: r.lifecycle_filter()?,
            },
        ),
        REQUEST_LIFECYCLE_UNSUBSCRIBE => RequestBody::LifecycleSubscription(
            ipp_core::systems::lifecycle_publisher::LifecyclePublisherCommand::Unsubscribe {
                subscription: r.lifecycle_subscription_id()?,
            },
        ),
        REQUEST_PLAYBACK => RequestBody::AnimationPlaybackCommand {
            controller: r.controller_id()?,
            control: r.playback_control()?,
        },
        REQUEST_CONTROLLER_CREATE => RequestBody::AnimationController(
            ipp_core::systems::animation::AnimationControllerCommand::Create(
                r.controller_description()?,
            ),
        ),
        REQUEST_CONTROLLER_UPDATE => RequestBody::AnimationController(
            ipp_core::systems::animation::AnimationControllerCommand::Update {
                id: r.controller_id()?,
                description: r.controller_description()?,
            },
        ),
        REQUEST_CONTROLLER_DELETE => RequestBody::AnimationController(
            ipp_core::systems::animation::AnimationControllerCommand::Delete {
                id: r.controller_id()?,
            },
        ),
        REQUEST_CONTROLLER_CONTROL => RequestBody::AnimationController(
            ipp_core::systems::animation::AnimationControllerCommand::Control {
                id: r.controller_id()?,
                control: r.playback_control()?,
            },
        ),
        REQUEST_CONTROLLER_TRANSITION => RequestBody::AnimationController(
            ipp_core::systems::animation::AnimationControllerCommand::Transition {
                id: r.controller_id()?,
                transition: r.controller_transition()?,
            },
        ),
        REQUEST_RENDER_STATE_UPDATE => {
            RequestBody::RenderStateUpdateCommand(r.render_state_patch()?)
        }
        REQUEST_CANVAS_STATE_UPDATE => {
            RequestBody::CanvasStateUpdateCommand(r.canvas_state_update()?)
        }
        REQUEST_GUI_PREFERENCES_UPDATE => {
            RequestBody::GuiPreferencesUpdateCommand(r.gui_preferences_update()?)
        }
        REQUEST_GEOMETRY_PICK => RequestBody::GeometryPickQuery(GeometryPickQuery {
            view: r.view_target()?,
            x: r.f32()?,
            y: r.f32()?,
            include_view_plane: r.boolean()?,
        }),
        REQUEST_CAMERA_PROJECT => RequestBody::CameraProjectQuery(CameraProjectQuery {
            view: r.view_target()?,
            x: r.f32()?,
            y: r.f32()?,
            plane: ipp_core::WorldPlane {
                point: [r.f32()?, r.f32()?, r.f32()?],
                normal: [r.f32()?, r.f32()?, r.f32()?],
            },
        }),
        REQUEST_CAMERA_NAVIGATE => {
            let binding = r.root_binding()?;
            let publication = r.view_source()?;
            let kind = r.u32()?;
            let first = r.f32()?;
            let second = r.f32()?;
            use ipp_core::systems::camera::CameraViewMotion;
            let motion = match kind {
                0 => CameraViewMotion::Rotate {
                    yaw: first,
                    pitch: second,
                },
                1 => CameraViewMotion::Pan {
                    x: first,
                    y: second,
                },
                2 if second == 0.0 => CameraViewMotion::Zoom {
                    amount: first,
                },
                _ => return Err(ProtocolError::Malformed("camera motion").into()),
            };
            RequestBody::CameraNavigate(CameraNavigateRequest {
                binding,
                publication,
                motion,
            })
        }
        tag => return Err(ProtocolError::Unsupported(tag).into()),
    };

    // Uncorrelated requests carry identity zero.
    let command = matches!(
        body,
        RequestBody::AnimationPlaybackCommand { .. } | RequestBody::RenderStateUpdateCommand(_)
    );
    let command = command
        || matches!(
            body,
            RequestBody::CanvasStateUpdateCommand(_) | RequestBody::GuiPreferencesUpdateCommand(_)
        );
    if command != (request_id == 0) {
        return Err(ProtocolError::Malformed("reserved request identity").into());
    }

    if r.at != bytes.len() {
        return Err(ProtocolError::Malformed("trailing bytes").into());
    }
    Ok(Request {
        session,
        request_id,
        body,
    })
}
