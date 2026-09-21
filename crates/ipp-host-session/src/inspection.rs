//! Bounded inspection at the completed frame boundary. Pages are independent reads.

use crate::{HostServices, WorldSessionContext};
use ipp_protocol::{InspectionQuery, Response, ResponseBody, encode_response};

impl<P: HostServices> WorldSessionContext<'_, P> {
    pub(crate) fn inspection_page(
        &self,
        query: InspectionQuery,
        request_id: u64,
        tick: u64,
        time: f64,
    ) -> ResponseBody {
        let count = query.limit as usize;
        let mut body = ResponseBody::Inspect {
            next: 0,
            time,
            entities: if query.collection == 1 {
                self.world.entity_page(query.after, query.target, count + 1)
            } else {
                vec![]
            },
            resources: if query.collection == 2 {
                self.world
                    .resource_page(query.after, query.target, count + 1)
            } else {
                vec![]
            },
            controllers: if query.collection == 3 {
                self.world
                    .animation_controller_page(query.after, query.target, count + 1)
            } else {
                vec![]
            },
            render_diagnostics: if query.collection == 4 {
                self.world
                    .render_diagnostic_page(query.after, query.target, count + 1)
            } else {
                vec![]
            },
        };
        let mut limit = count;
        loop {
            let ResponseBody::Inspect {
                next,
                entities,
                resources,
                controllers,
                render_diagnostics,
                ..
            } = &mut body
            else {
                unreachable!()
            };
            macro_rules! trim {
                ($items:ident, $identity:expr) => {
                    if $items.len() > limit {
                        $items.truncate(limit);
                        *next = $items.last().map($identity).unwrap_or(0);
                    }
                };
            }
            trim!(entities, |item| item.id.to_bits());
            trim!(resources, |item| item.id);
            trim!(controllers, |item| item.id.to_bits());
            trim!(render_diagnostics, |item| item.entity.to_bits());
            let response = Response {
                session: self.session.id,
                request_id,
                tick,
                body,
            };
            match encode_response(&response) {
                Ok(_) => return response.body,
                Err(ipp_protocol::ProtocolError::Limit(_)) if limit > 1 => {
                    limit /= 2;
                    body = response.body;
                }
                Err(error) => {
                    return ResponseBody::Error {
                        code: 1,
                        message: format!("Inspection record cannot be encoded: {error}"),
                    };
                }
            }
        }
    }
}
