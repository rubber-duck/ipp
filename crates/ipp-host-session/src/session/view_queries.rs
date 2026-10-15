//! Resolve session-fenced queries only after the Host completes ordered World evaluation.

use crate::*;

#[cfg(test)]
#[path = "view_queries_tests.rs"]
mod tests;

impl<P: HostServices> WorldSessionContext<'_, P> {
    pub(crate) fn fail_view_queries(
        &mut self,
        reason: ipp_core::ErrorReason,
    ) -> Result<(), String> {
        let mut pending = std::mem::take(&mut self.session.replies);
        for (request_id, reply) in pending.drain(..) {
            let body = match reply {
                WorldSessionReply::GeometryPick(_) => ResponseBody::GeometryPickResultEvent(
                    ipp_protocol::world::view_queries::ViewQueryOutcome {
                        request_id,
                        tick: self.world.tick(),
                        result: Err(reason),
                    },
                ),
                WorldSessionReply::CameraNavigate(_) => ResponseBody::Error {
                    code: 1,
                    message: reason.to_string(),
                },
                WorldSessionReply::CompletedCameraNavigate {
                    body,
                    tick,
                } => {
                    self.queue_response_at(request_id, body, tick)?;
                    continue;
                }
                WorldSessionReply::CameraProject(_) => ResponseBody::CameraProjectResultEvent(
                    ipp_protocol::world::view_queries::ViewQueryOutcome {
                        request_id,
                        tick: self.world.tick(),
                        result: Err(reason),
                    },
                ),
                retained => {
                    self.session.replies.push((request_id, retained));
                    continue;
                }
            };
            self.queue_response(request_id, body)?;
        }
        self.session.prepared = !self.session.replies.is_empty();
        Ok(())
    }
}

impl<P: HostServices> Host<P> {
    pub(crate) fn admit_camera_navigation(
        &mut self,
        id: u64,
        position: usize,
    ) -> Result<bool, String> {
        let world_id = self.sessions[&id].world;
        let world = self.runtime.world_ref(world_id).ok_or("World is closed")?;
        let request = self
            .sessions
            .get_mut(&id)
            .unwrap()
            .pending
            .remove(position)
            .unwrap();
        let RequestBody::CameraNavigate(navigation) = request.body else {
            unreachable!("navigation request");
        };
        let mut producer = world_id;
        let result = navigation
            .resolve(&self.runtime, world)
            .and_then(|command| {
                producer = command.output().world().id();
                self.runtime
                    .world_mut(producer)
                    .ok_or(ipp_core::ErrorReason::InvalidEntity)?
                    .enqueue_system_command_with_reply(
                        ipp_core::systems::camera::CameraSystem::ID,
                        id,
                        request.request_id,
                        command,
                    )
            });
        let session = self.sessions.get_mut(&id).unwrap();
        session.replies.push((
            request.request_id,
            match result {
                Ok(()) => WorldSessionReply::CameraNavigate(producer),
                Err(error) => WorldSessionReply::Rejected(error.to_string()),
            },
        ));
        session.prepared = true;
        Ok(true)
    }

    pub(crate) fn complete_view_queries(&mut self, frame: &ipp_core::HostFrameReport) {
        for session in self
            .sessions
            .values_mut()
            .filter(|session| session.prepared)
        {
            let Some(world) = self.runtime.world_ref(session.world) else {
                continue;
            };
            let Some(Ok(report)) = frame.worlds.get(&session.world) else {
                continue;
            };
            for (request_id, reply) in &mut session.replies {
                let body = match reply {
                    WorldSessionReply::CameraNavigate(producer) => {
                        let (tick, body) = match frame.worlds.get(producer) {
                            Some(Ok(producer_report)) => {
                                let result = producer_report.system_command_outcomes.iter().find(
                                    |outcome| {
                                        outcome.session == session.id
                                            && outcome.request_id == *request_id
                                    },
                                );
                                let body = match result.map(|outcome| outcome.result) {
                                    Some(Ok(())) => ResponseBody::CameraNavigated,
                                    Some(Err(reason)) => ResponseBody::Error {
                                        code: 1,
                                        message: reason.to_string(),
                                    },
                                    None => ResponseBody::Error {
                                        code: 1,
                                        message: "Camera producer did not complete navigation"
                                            .into(),
                                    },
                                };
                                (report.tick, body)
                            }
                            _ => (
                                report.tick,
                                ResponseBody::Error {
                                    code: 1,
                                    message: "Camera producer is unavailable".into(),
                                },
                            ),
                        };
                        *reply = WorldSessionReply::CompletedCameraNavigate {
                            body,
                            tick,
                        };
                        continue;
                    }
                    WorldSessionReply::GeometryPick(query) => {
                        ResponseBody::GeometryPickResultEvent(
                            ipp_protocol::world::view_queries::ViewQueryOutcome {
                                request_id: *request_id,
                                tick: report.tick,
                                result: query.view.pick(
                                    &self.runtime,
                                    world,
                                    [query.x, query.y],
                                    query.include_view_plane,
                                ),
                            },
                        )
                    }
                    WorldSessionReply::CameraProject(query) => {
                        ResponseBody::CameraProjectResultEvent(
                            ipp_protocol::world::view_queries::ViewQueryOutcome {
                                request_id: *request_id,
                                tick: report.tick,
                                result: query.view.project(
                                    &self.runtime,
                                    world,
                                    [query.x, query.y],
                                    query.plane,
                                ),
                            },
                        )
                    }
                    _ => continue,
                };
                *reply = WorldSessionReply::CompletedView(body);
            }
        }
    }
}
