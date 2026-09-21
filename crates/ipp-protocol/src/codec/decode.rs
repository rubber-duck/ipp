use super::*;

impl<'a> Reader<'a> {
    pub(super) fn f64(&mut self) -> Result<f64, ProtocolError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .at
            .checked_add(n)
            .ok_or(ProtocolError::Limit("length"))?;
        let v = self
            .bytes
            .get(self.at..end)
            .ok_or(ProtocolError::Malformed("truncated"))?;
        self.at = end;
        Ok(v)
    }

    pub(crate) fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn boolean(&mut self) -> Result<bool, ProtocolError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(ProtocolError::Malformed("boolean encoding")),
        }
    }

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

    pub(crate) fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub(crate) fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub(crate) fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub(super) fn f32(&mut self) -> Result<f32, ProtocolError> {
        let v = f32::from_bits(self.u32()?);
        if v.is_finite() {
            Ok(v)
        } else {
            Err(ProtocolError::Malformed("nonfinite f32"))
        }
    }

    pub(crate) fn count(&mut self, max: usize) -> Result<usize, ProtocolError> {
        let n = self.u32()? as usize;
        if n > max {
            Err(ProtocolError::Limit("count"))
        } else {
            Ok(n)
        }
    }

    pub(crate) fn string(&mut self) -> Result<String, ProtocolError> {
        let n = self.count(65536)?;
        std::str::from_utf8(self.take(n)?)
            .map(str::to_owned)
            .map_err(|_| ProtocolError::Malformed("utf8"))
    }

    pub(crate) fn bytes(&mut self) -> Result<Vec<u8>, ProtocolError> {
        self.bytes_bounded(65536)
    }

    pub(crate) fn bytes_bounded(&mut self, max: usize) -> Result<Vec<u8>, ProtocolError> {
        let n = self.count(max)?;
        Ok(self.take(n)?.to_vec())
    }

    pub(super) fn metadata(&mut self) -> Result<EntityMetadata, ProtocolError> {
        let symbolic_id = match self.u8()? {
            OPTION_NONE => None,
            OPTION_SOME => Some(self.string()?),
            _ => return Err(ProtocolError::Malformed("option tag")),
        };
        let n = self.count(256)?;
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
        let value = match self.u8()? {
            VALUE_BOOL => FieldValue::Bool(self.boolean()?),
            VALUE_F32 => FieldValue::F32(self.f32()?),
            VALUE_ENTITY => FieldValue::Entity(self.entity()?),
            VALUE_U32 => FieldValue::U32(self.u32()?),
            VALUE_U64 => FieldValue::U64(self.u64()?),
            VALUE_STRING => FieldValue::String(self.string()?),
            VALUE_BYTES => FieldValue::Bytes(self.bytes()?),
            VALUE_DYNAMIC => FieldValue::Dynamic(
                ipp_core::DynamicValue::decode(&self.bytes()?)
                    .map_err(|_| ProtocolError::Malformed("dynamic value"))?,
            ),
            tag => return Err(ProtocolError::Unsupported(tag)),
        };
        Ok(FieldWrite {
            offset,
            value,
        })
    }

    pub(super) fn resource(&mut self) -> Result<ipp_core::StateOverlayRef, ProtocolError> {
        Ok(match self.u8()? {
            REF_HANDLE => ipp_core::StateOverlayRef::Handle(self.u64()?),
            REF_ALIAS => ipp_core::StateOverlayRef::Alias(self.u32()?),
            tag => return Err(ProtocolError::Unsupported(tag)),
        })
    }

    pub(super) fn fields(&mut self) -> Result<Vec<FieldWrite>, ProtocolError> {
        let n = self.count(256)?;
        (0..n).map(|_| self.field()).collect()
    }

    pub(super) fn command(&mut self) -> Result<Command, ProtocolError> {
        Ok(match self.u8()? {
            COMMAND_CREATE => Command::Create {
                alias: self.u32()?,
                metadata: self.metadata()?,
            },
            COMMAND_DELETE => Command::Delete {
                entity: self.entity()?,
            },
            COMMAND_METADATA => Command::SetMetadata {
                entity: self.entity()?,
                metadata: self.metadata()?,
            },
            COMMAND_INSERT => {
                let entity = self.entity()?;
                let component = self.u16()?;
                let n = self.count(256)?;
                let mut fields = Vec::with_capacity(n);
                for _ in 0..n {
                    fields.push(self.field()?);
                }
                Command::InsertComponent {
                    entity,
                    component,
                    fields,
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
            COMMAND_UPDATE_DYNAMIC_COMPONENT_STATE_OVERLAY => {
                let owner = self.resource()?;
                let overlay = self.resource()?;
                let count = self.count(65536)?;
                let properties = (0..count)
                    .map(|_| {
                        Ok((
                            self.string()?,
                            ipp_core::DynamicValue::decode(&self.bytes()?)
                                .map_err(|_| ProtocolError::Malformed("dynamic value"))?,
                        ))
                    })
                    .collect::<Result<Vec<_>, ProtocolError>>()?;
                let count = self.count(65536)?;
                let clear = (0..count)
                    .map(|_| self.string())
                    .collect::<Result<Vec<_>, _>>()?;
                Command::UpdateDynamicComponentStateOverlay {
                    owner,
                    overlay,
                    properties,
                    clear,
                }
            }
            COMMAND_SET => Command::SetField {
                entity: self.entity()?,
                component: self.u16()?,
                field: self.field()?,
            },
            COMMAND_REMOVE => Command::RemoveComponent {
                entity: self.entity()?,
                component: self.u16()?,
            },
            COMMAND_CREATE_STATE_OVERLAY_OWNER => Command::CreateStateOverlayOwner {
                alias: self.u32()?,
            },
            COMMAND_RELEASE_STATE_OVERLAY_OWNER => Command::ReleaseStateOverlayOwner {
                owner: self.resource()?,
            },
            COMMAND_ATTACH_ENTITY_OVERLAY_BINDING => Command::AttachEntityOverlayBinding {
                owner: self.resource()?,
                alias: self.u32()?,
                symbolic_id: self.string()?,
                mode: match self.u8()? {
                    ENTITY_OVERLAY_MODE_OWNED => ipp_core::EntityOverlayMode::Owned,
                    ENTITY_OVERLAY_MODE_BOUND => ipp_core::EntityOverlayMode::Bound,
                    _ => return Err(ProtocolError::Malformed("entity mode")),
                },
            },
            COMMAND_RELEASE_ENTITY_OVERLAY_BINDING => Command::ReleaseEntityOverlayBinding {
                owner: self.resource()?,
                binding: self.resource()?,
            },
            COMMAND_ATTACH_COMPONENT_STATE_OVERLAY => Command::AttachComponentStateOverlay {
                owner: self.resource()?,
                binding: self.resource()?,
                alias: self.u32()?,
                component: self.u16()?,
                mode: match self.u8()? {
                    COMPONENT_OVERLAY_MODE_AUTO => ipp_core::ComponentOverlayMode::Auto,
                    COMPONENT_OVERLAY_MODE_BOUND => ipp_core::ComponentOverlayMode::Bound,
                    COMPONENT_OVERLAY_MODE_OWNED => ipp_core::ComponentOverlayMode::Owned,
                    _ => return Err(ProtocolError::Malformed("component mode")),
                },
                fields: self.fields()?,
            },
            COMMAND_UPDATE_COMPONENT_STATE_OVERLAY => {
                let owner = self.resource()?;
                let overlay = self.resource()?;
                let fields = self.fields()?;
                let n = self.count(256)?;
                let clear = (0..n).map(|_| self.u32()).collect::<Result<_, _>>()?;
                Command::UpdateComponentStateOverlay {
                    owner,
                    overlay,
                    fields,
                    clear,
                }
            }
            COMMAND_RELEASE_COMPONENT_STATE_OVERLAY => Command::ReleaseComponentStateOverlay {
                owner: self.resource()?,
                overlay: self.resource()?,
            },
            tag => return Err(ProtocolError::Unsupported(tag)),
        })
    }
}

/// Decode one bounded complete message into owned core commands.
/// Call only after `accept_bootstrap` succeeds for this connection.
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
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(ProtocolError::Limit("message"));
    }

    let mut r = Reader {
        bytes,
        at: 0,
    };
    let session = r.u64()?;
    if session == 0 || session != expected_session {
        return Err(ProtocolError::SessionMismatch);
    }

    let request_id = r.u64()?;

    let body = match r.u8()? {
        #[cfg(feature = "surfaces")]
        REQUEST_SURFACE => RequestBody::SurfaceCommand(r.surface_command()?),
        #[cfg(feature = "gui")]
        REQUEST_GUI => RequestBody::GuiCommands {
            batch_id: r.boolean()?.then(|| r.u64()).transpose()?,
            commands: r.gui_commands()?,
        },
        #[cfg(feature = "gui")]
        REQUEST_GUI_INSPECT => RequestBody::GuiInspect(r.gui_inspect_query()?),
        #[cfg(feature = "gui")]
        REQUEST_GUI_INPUT => RequestBody::GuiInput(Box::new(r.gui_input_command()?)),
        #[cfg(feature = "gui")]
        REQUEST_GUI_SEMANTIC_SNAPSHOT => {
            RequestBody::GuiSemanticSnapshot(r.gui_semantic_snapshot_query()?)
        }
        #[cfg(feature = "gui")]
        REQUEST_GUI_SEMANTIC_ACTION => {
            RequestBody::GuiSemanticAction(Box::new(r.gui_semantic_action()?))
        }
        REQUEST_END_BATCH => RequestBody::EndBatch(r.u64()?),
        REQUEST_BEGIN_BATCH => RequestBody::BeginBatch,
        tag @ (REQUEST_BATCH | REQUEST_BATCH_CHUNK) => {
            let id = r.u64()?;
            if bytes.len() > 128 * 1024 {
                return Err(ProtocolError::Limit("command page bytes"));
            }
            let n = r.count(256)?;
            operations.clear();
            // Geometric growth could retain more than the wire limit for later batches.
            operations.reserve_exact(n);
            for _ in 0..n {
                operations.push(r.command()?);
            }
            let batch = Batch {
                id,
                operations: std::mem::take(operations),
            };
            if tag == REQUEST_BATCH_CHUNK {
                RequestBody::BatchChunk(batch)
            } else {
                RequestBody::Batch(batch)
            }
        }
        REQUEST_INSPECT => {
            let collection = r.u8()?;
            let after = r.u64()?;
            let target = r.u64()?;
            let limit = r.u16()?;
            if collection > 4 || limit == 0 || limit > 256 || (target != 0 && after != 0) {
                return Err(ProtocolError::Malformed("inspection query"));
            }
            RequestBody::Inspect(crate::InspectionQuery {
                collection,
                after,
                target,
                limit,
            })
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
        REQUEST_CAMERA_ACTIVATE => RequestBody::CameraActivateCommand {
            entity: EntityId::from_bits(r.u64()?),
        },
        REQUEST_CAMERA_NAVIGATE => RequestBody::CameraNavigateCommand(match r.u8()? {
            CAMERA_MOTION_ROTATE => ipp_core::CameraMotion::Rotate {
                yaw: r.f32()?,
                pitch: r.f32()?,
            },
            CAMERA_MOTION_PAN => ipp_core::CameraMotion::Pan {
                x: r.f32()?,
                y: r.f32()?,
                width: r.u32()?,
                height: r.u32()?,
            },
            CAMERA_MOTION_ZOOM => ipp_core::CameraMotion::Zoom {
                amount: r.f32()?,
            },
            _ => return Err(ProtocolError::Malformed("camera motion tag")),
        }),
        REQUEST_GEOMETRY_PICK => RequestBody::GeometryPickQuery(ipp_core::GeometryPickQuery {
            x: r.f32()?,
            y: r.f32()?,
            width: r.u32()?,
            height: r.u32()?,
            include_view_plane: r.boolean()?,
        }),
        REQUEST_CAMERA_PROJECT => RequestBody::CameraProjectQuery(ipp_core::CameraProjectQuery {
            x: r.f32()?,
            y: r.f32()?,
            width: r.u32()?,
            height: r.u32()?,
            plane: ipp_core::WorldPlane {
                point: [r.f32()?, r.f32()?, r.f32()?],
                normal: [r.f32()?, r.f32()?, r.f32()?],
            },
        }),
        tag => return Err(ProtocolError::Unsupported(tag)),
    };

    let command = matches!(
        body,
        RequestBody::AnimationPlaybackCommand { .. }
            | RequestBody::CameraActivateCommand { .. }
            | RequestBody::CameraNavigateCommand(_)
            | RequestBody::RenderStateUpdateCommand(_)
    );
    if command != (request_id == 0) {
        return Err(ProtocolError::Malformed("reserved request identity"));
    }

    if r.at != bytes.len() {
        return Err(ProtocolError::Malformed("trailing bytes"));
    }
    Ok(Request {
        session,
        request_id,
        body,
    })
}
