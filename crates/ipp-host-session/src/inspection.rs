//! Bounded inspection at the completed frame boundary. Pages are independent reads.

use crate::{HostServices, WorldSessionContext};
use ipp_core::{EntityId, WorldContext};
use ipp_protocol::{EntityTreeNode, InspectionQuery, Response, ResponseBody, encode_response};

#[derive(Clone)]
pub(crate) struct EntityTreeCursor<'a, 'world> {
    world: &'a WorldContext<'world>,
    root: Option<EntityId>,
    max_depth: u16,
    current: Option<(EntityId, u16)>,
}

impl<'a, 'world> EntityTreeCursor<'a, 'world> {
    pub(crate) fn new(
        world: &'a WorldContext<'world>,
        query: &InspectionQuery,
    ) -> Result<Self, &'static str> {
        let root = (query.target != 0).then(|| EntityId::from_bits(query.target));
        if root.is_some_and(|entity| world.entity_link(entity).is_none()) {
            return Err("Tree root no longer exists");
        }
        let mut cursor = Self {
            world,
            root,
            max_depth: query.max_depth,
            current: root
                .or_else(|| world.entity_children(None).next())
                .map(|entity| (entity, 0)),
        };
        if query.after != 0 {
            let entity = EntityId::from_bits(query.after);
            let mut ancestor = Some(entity);
            let mut depth = 0;
            loop {
                let ancestor_entity =
                    ancestor.ok_or("Tree cursor is stale or outside the requested root")?;
                let link = world
                    .entity_link(ancestor_entity)
                    .ok_or("Tree cursor is stale or outside the requested root")?;
                if root == Some(ancestor_entity) || (root.is_none() && link.parent.is_none()) {
                    break;
                }
                depth += 1;
                if depth > query.max_depth {
                    return Err("Tree cursor exceeds the requested depth");
                }
                ancestor = link.parent;
            }
            cursor.current = cursor.successor(entity, depth);
        }
        Ok(cursor)
    }

    fn successor(&self, entity: EntityId, depth: u16) -> Option<(EntityId, u16)> {
        if depth < self.max_depth
            && let Some(child) = self.world.entity_children(Some(entity)).next()
        {
            return Some((child, depth + 1));
        }
        let mut cursor = entity;
        let mut cursor_depth = depth;
        loop {
            if self.root == Some(cursor) {
                return None;
            }
            let parent = self.world.entity_link(cursor)?.parent;
            if let Some(sibling) = self.world.entity_next_sibling(cursor) {
                return Some((sibling, cursor_depth));
            }
            cursor = parent?;
            cursor_depth -= 1;
        }
    }
}

impl Iterator for EntityTreeCursor<'_, '_> {
    type Item = EntityTreeNode;

    fn next(&mut self) -> Option<Self::Item> {
        let (entity, depth) = self.current?;
        let link = self
            .world
            .entity_link(entity)
            .expect("immutable indexed tree entity");
        self.current = self.successor(entity, depth);
        Some(EntityTreeNode {
            id: entity,
            parent: link.parent,
            order: link.order.value(),
            depth,
        })
    }
}

/// Trim a `GuiPointers` page at a whole target entity, so the entity cursor
/// never skips a pointer of the last entity. A single entity with more
/// records than a page stays whole.
fn trim_pointer_page(
    records: &mut Vec<ipp_core::systems::gui::local::GuiPointerRecord>,
    limit: usize,
    next: &mut u64,
) {
    if records.len() <= limit {
        return;
    }
    let cut = records[limit].target.entity;
    let end = match records[..limit]
        .iter()
        .rposition(|record| record.target.entity != cut)
    {
        Some(index) => index + 1,
        None => records
            .iter()
            .position(|record| record.target.entity != cut)
            .unwrap_or(records.len()),
    };
    if end < records.len() {
        records.truncate(end);
        *next = records
            .last()
            .map_or(0, |record| record.target.entity.to_bits());
    }
}

impl<P: HostServices> WorldSessionContext<'_, P> {
    pub(crate) fn inspection_page(
        &self,
        query: InspectionQuery,
        request_id: u64,
        tick: u64,
        time: f64,
    ) -> ResponseBody {
        if query.collection == 5 {
            return self.entity_tree_page(query, request_id, tick, time);
        }
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
            gui_focus: if query.collection == 6 {
                self.world
                    .gui_focus_page(query.after, query.target, count + 1)
            } else {
                vec![]
            },
            gui_pointers: if query.collection == 7 {
                self.world
                    .gui_pointer_page(query.after, query.target, count + 1)
            } else {
                vec![]
            },
            gui_active_items: if query.collection == 9 {
                self.world
                    .gui_active_item_page(query.after, query.target, count + 1)
            } else {
                vec![]
            },
            canvas: if query.collection == 8 {
                self.world.canvas_state().ok()
            } else {
                None
            },
            gui_preferences: if query.collection == 10 {
                self.world.gui_preferences().ok()
            } else {
                None
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
                gui_focus,
                gui_pointers,
                gui_active_items,
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
            trim!(gui_focus, |item| item.target.entity.to_bits());
            trim!(gui_active_items, |item| item.group.to_bits());
            trim_pointer_page(gui_pointers, limit, next);
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

    fn entity_tree_page(
        &self,
        query: InspectionQuery,
        request_id: u64,
        tick: u64,
        time: f64,
    ) -> ResponseBody {
        let cursor = match EntityTreeCursor::new(&self.world, &query) {
            Ok(cursor) => cursor,
            Err(message) => {
                return ResponseBody::Error {
                    code: 1,
                    message: message.into(),
                };
            }
        };
        let nodes: Vec<_> = cursor.take(query.limit as usize + 1).collect();

        let mut limit = query.limit as usize;
        loop {
            let next = if nodes.len() > limit {
                nodes[limit - 1].id.to_bits()
            } else {
                0
            };
            let response = Response {
                session: self.session.id,
                request_id,
                tick,
                body: ResponseBody::EntityTree {
                    next,
                    time,
                    nodes: nodes[..nodes.len().min(limit)].to_vec(),
                },
            };
            match encode_response(&response) {
                Ok(_) => return response.body,
                Err(ipp_protocol::ProtocolError::Limit(_)) if limit > 1 => limit /= 2,
                Err(error) => {
                    return ResponseBody::Error {
                        code: 1,
                        message: format!("Tree page cannot be encoded: {error}"),
                    };
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "inspection_tests.rs"]
mod tests;
