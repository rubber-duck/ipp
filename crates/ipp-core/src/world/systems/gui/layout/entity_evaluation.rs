use super::super::local::control::{component_incarnation, eligibility};
use super::component::GuiLayout;
use super::entity_layout::{GuiEntityLayout, GuiEntityLayoutDiagnostic, GuiEntityLayoutView};
use super::geometry::{Constraints, align_factor, fill_or_fit};
use super::scroll_layout::{GuiScrollFieldPosition, GuiScrollLayout};
use crate::systems::SystemRuntimeAccess;
use crate::systems::canvas::{CanvasGeometry, CanvasTarget, prepare_constrained_geometry};
use crate::{ComponentValue, EntityId};
use std::collections::{BTreeMap, BTreeSet};

/// Components that make an entity a managed layout box.
const LAYOUT_COMPONENTS: [u16; 7] = [
    ComponentValue::GUI_LAYOUT,
    ComponentValue::GUI_BUTTON,
    ComponentValue::GUI_CHECKBOX,
    ComponentValue::GUI_SLIDER,
    ComponentValue::GUI_TEXT_INPUT,
    ComponentValue::GUI_SCROLL_VIEW,
    ComponentValue::GUI_VIRTUAL_LIST,
];

/// Maximum tree depth followed during evaluation. Deeper subtrees are cut
/// with a diagnostic instead of recursing without bound.
pub const MAX_LAYOUT_DEPTH: usize = 128;

struct Evaluator<'context, 'world> {
    context: &'context SystemRuntimeAccess<'world>,
    previous: Option<&'context GuiEntityLayoutView>,
    dirty: &'context BTreeSet<EntityId>,
    resources_dirty: bool,
    gui: Option<&'context super::super::GuiSystem>,
    /// Nearest entity at or above the current one that declares `GuiFont`.
    font: Option<EntityId>,
    view: GuiEntityLayoutView,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate(
    context: &SystemRuntimeAccess<'_>,
    extent: [f32; 2],
    density: f32,
    previous: Option<&GuiEntityLayoutView>,
    dirty: &BTreeSet<EntityId>,
    resources_dirty: bool,
    revision: u64,
    gui: Option<&super::super::GuiSystem>,
) -> GuiEntityLayoutView {
    let mut evaluator = Evaluator {
        context,
        previous,
        dirty,
        resources_dirty,
        gui,
        font: None,
        view: GuiEntityLayoutView {
            revision,
            extent,
            density,
            placements: BTreeMap::new(),
            geometry: BTreeMap::new(),
            text_constraints: BTreeMap::new(),
            control_labels: BTreeMap::new(),
            scrolls: BTreeMap::new(),
            diagnostics: Vec::new(),
            work: super::entity_layout::GuiEntityLayoutWork {
                reflows: 1,
                ..Default::default()
            },
        },
    };
    // Each top-level entity is a layout root within the canvas extent.
    let mut pending: Vec<_> = context
        .world
        .state
        .links
        .children(None)
        .map(|entity| (entity, None))
        .collect();
    while let Some((entity, font)) = pending.pop() {
        if !evaluator.in_scope(entity) {
            continue;
        }
        evaluator.font = font;
        if evaluator.declared(entity) {
            evaluator.visit(entity, Constraints::loose(extent[0], extent[1]), 0);
            let margin = evaluator.style(entity).margin();
            if let Some(placement) = evaluator.view.placements.get_mut(&entity) {
                placement.origin = [margin[3], margin[0]];
            }
        } else {
            let font = evaluator.font_owner(entity);
            pending.extend(
                evaluator
                    .children(entity)
                    .into_iter()
                    .map(|child| (child, font)),
            );
        }
    }
    evaluator.view
}

impl Evaluator<'_, '_> {
    fn in_scope(&self, entity: EntityId) -> bool {
        !self.context.world.state.links.invalid.contains(&entity)
    }

    fn declared(&self, entity: EntityId) -> bool {
        LAYOUT_COMPONENTS.iter().any(|component| {
            component_incarnation(&self.context.world.state, entity, *component).is_some()
        })
    }

    /// The typography owner inherited by `entity`'s content.
    fn font_owner(&self, entity: EntityId) -> Option<EntityId> {
        if self
            .context
            .world
            .components
            .gui_font(entity.index() as usize)
            .is_some()
        {
            Some(entity)
        } else {
            self.font
        }
    }

    fn style(&self, entity: EntityId) -> GuiLayout {
        self.context
            .world
            .components
            .gui_layout(entity.index() as usize)
            .copied()
            .unwrap_or_default()
    }

    fn children(&self, entity: EntityId) -> Vec<EntityId> {
        self.context
            .world
            .state
            .links
            .children(Some(entity))
            .filter(|child| self.in_scope(*child))
            .collect()
    }

    fn place(&mut self, entity: EntityId, origin: [f32; 2]) {
        if let Some(placement) = self.view.placements.get_mut(&entity) {
            placement.origin = origin;
        }
    }

    fn visit(&mut self, entity: EntityId, constraints: Constraints, depth: usize) -> [f32; 2] {
        self.view.work.visited_entities += 1;

        if depth > MAX_LAYOUT_DEPTH {
            self.view
                .diagnostics
                .push(GuiEntityLayoutDiagnostic::DepthLimit {
                    entity,
                    limit: MAX_LAYOUT_DEPTH,
                });
            self.view.placements.insert(
                entity,
                GuiEntityLayout {
                    origin: [0.0; 2],
                    size: [0.0; 2],
                    content_offset: [0.0; 2],
                    clip: true,
                    available: false,
                },
            );
            return [0.0; 2];
        }
        let inherited_font = self.font;
        self.font = self.font_owner(entity);
        let size = self.visit_box(entity, constraints, depth);
        self.font = inherited_font;
        size
    }

    fn visit_box(&mut self, entity: EntityId, constraints: Constraints, depth: usize) -> [f32; 2] {
        let style = self.style(entity);
        let scrolling = GuiScrollFieldPosition::read(&self.context.world.components, entity)
            .and_then(|(component, position)| {
                Some(GuiScrolling {
                    component,
                    incarnation: component_incarnation(
                        &self.context.world.state,
                        entity,
                        component,
                    )?,
                    position,
                    available: eligibility(self.context.world, entity).available,
                })
            });
        let padding = style.padding();
        let explicit = [optional(style.width), optional(style.height)];
        let maximum = [optional(style.max_width), optional(style.max_height)];
        let mut fill = [
            constraints.max_w.min(explicit[0]).min(maximum[0]),
            constraints.max_h.min(explicit[1]).min(maximum[1]),
        ];
        if scrolling.is_some() {
            fill[0] = constraints.clamp_width(fill[0].clamp(style.min_width, maximum[0]));
            fill[1] = constraints.clamp_height(fill[1].clamp(style.min_height, maximum[1]));
        }
        let content = Constraints::loose(
            (fill[0] - padding[1] - padding[3]).max(0.0),
            (fill[1] - padding[0] - padding[2]).max(0.0),
        );
        let natural = self.intrinsic(entity, content.max_w);
        let children = self.children(entity);
        let mut size = if let Some(control) = &scrolling {
            self.scroll(entity, &children, content, style, depth, control)
        } else {
            match style.kind {
                1 | 2 => self.flex(&children, content, fill, style.kind == 1, depth),
                3 => self.stack(&children, content, fill, depth),
                4..=6 => self.single(&children, style, content, fill, depth),
                _ => {
                    for &child in &children {
                        self.visit(child, content, depth + 1);
                    }
                    [
                        content.clamp_width(natural[0]),
                        content.clamp_height(natural[1]),
                    ]
                }
            }
        };
        for child in children {
            if let Some(placement) = self.view.placements.get_mut(&child) {
                placement.origin[0] += padding[3];
                placement.origin[1] += padding[0];
            }
        }
        if style.width >= 0.0 {
            size[0] = style.width;
        }
        if style.height >= 0.0 {
            size[1] = style.height;
        }
        size[0] = constraints.clamp_width(size[0].clamp(style.min_width, maximum[0]));
        size[1] = constraints.clamp_height(size[1].clamp(style.min_height, maximum[1]));
        let available = size.iter().all(|value| value.is_finite() && *value >= 0.0)
            && scrolling.as_ref().is_none_or(|control| control.available)
            && (scrolling.is_none() || fill.into_iter().all(f32::is_finite));
        self.view.placements.insert(
            entity,
            GuiEntityLayout {
                origin: [0.0; 2],
                size,
                content_offset: [padding[3], padding[0]],
                clip: style.clip || scrolling.is_some(),
                available,
            },
        );
        size
    }

    fn scroll(
        &mut self,
        entity: EntityId,
        children: &[EntityId],
        constraints: Constraints,
        style: GuiLayout,
        depth: usize,
        control: &GuiScrolling,
    ) -> [f32; 2] {
        let viewport = [constraints.max_w, constraints.max_h];
        if !control.available || !viewport.into_iter().all(f32::is_finite) {
            return [0.0; 2];
        }
        let world = &*self.context.world;
        let list_config = world
            .components
            .gui_virtual_list(entity.index() as usize)
            .copied();
        let axis = list_config
            .map(|config| config.axis)
            .or_else(|| {
                world
                    .components
                    .gui_scroll_view(entity.index() as usize)
                    .map(|config| config.axis)
            })
            .unwrap_or(1);
        let child_constraints = Constraints::loose(
            if axis == 0 || axis == 2 {
                f32::INFINITY
            } else {
                viewport[0]
            },
            if axis == 1 || axis == 2 {
                f32::INFINITY
            } else {
                viewport[1]
            },
        );
        let mut list = list_config.map(|config| {
            super::virtual_list::GuiVirtualListLayout::from_parameters(
                config.item_count,
                config.item_extent,
                config.overscan,
                axis as usize,
                viewport,
            )
        });
        let mut content = [0.0_f32; 2];
        if let Some(list) = &mut list {
            let mut indexed: Vec<_> = children
                .iter()
                .filter_map(|&child| {
                    self.context
                        .world
                        .components
                        .gui_virtual_item(child.index() as usize)
                        .map(|item| (item.index, child))
                })
                .collect();
            indexed.sort_by_key(|&(index, _)| index);
            let mut accepted = BTreeSet::new();
            for (index, child) in indexed {
                if let Some(index) = list.accepts(index) {
                    let size = self.visit(child, child_constraints, depth + 1);
                    let position = list.next_position(index);
                    self.place(child, list.local_point(position));
                    list.push(index, child, size, size.into_iter().all(f32::is_finite));
                    accepted.insert(child);
                }
            }
            for &child in children {
                if !accepted.contains(&child) {
                    self.view.placements.insert(
                        child,
                        GuiEntityLayout {
                            origin: [0.0; 2],
                            size: [0.0; 2],
                            content_offset: [0.0; 2],
                            clip: true,
                            available: false,
                        },
                    );
                }
            }
            content = list.content_size(viewport);
        } else if let Some((&child, extra)) = children.split_first() {
            content = self.visit(child, child_constraints, depth + 1);
            self.place(child, [0.0; 2]);
            for &child in extra {
                self.view
                    .diagnostics
                    .push(GuiEntityLayoutDiagnostic::ExtraChild {
                        entity: child,
                    });
                self.view.placements.insert(
                    child,
                    GuiEntityLayout {
                        origin: [0.0; 2],
                        size: [0.0; 2],
                        content_offset: [0.0; 2],
                        clip: true,
                        available: false,
                    },
                );
            }
        }
        let settled = GuiScrollLayout::settle(
            control.component,
            control.incarnation,
            control.position,
            self.previous.and_then(|view| view.scrolls.get(&entity)),
            viewport,
            content,
            axis,
            list,
        );
        for child in children {
            if let Some(placement) = self.view.placements.get_mut(child) {
                for axis in 0..2 {
                    placement.origin[axis] -= settled.offset[axis];
                }
            }
        }
        self.view.scrolls.insert(entity, settled);
        let padding = style.padding();
        [
            viewport[0] + padding[1] + padding[3],
            viewport[1] + padding[0] + padding[2],
        ]
    }

    fn intrinsic(&mut self, entity: EntityId, max_width: f32) -> [f32; 2] {
        let world = &*self.context.world;
        let mut size =
            world
                .components
                .surface(entity.index() as usize)
                .map_or([0.0; 2], |surface| {
                    [
                        surface.width * self.view.density,
                        surface.height * self.view.density,
                    ]
                });
        if let Some(gui) = self.gui
            && let Some((label, measured)) =
                super::super::presentation::measurement::measure_control(
                    self.context,
                    gui,
                    entity,
                    self.font
                        .and_then(|owner| world.components.gui_font(owner.index() as usize)),
                    self.previous
                        .and_then(|view| view.control_labels.get(&entity)),
                )
        {
            size = [
                size[0].max(label.intrinsic[0]),
                size[1].max(label.intrinsic[1]),
            ];
            if measured {
                self.view.work.text_measurements += 1;
            } else if label.geometry.is_some() {
                self.view.work.reused_texts += 1;
            }
            self.view.control_labels.insert(entity, label);
        }
        for component in [
            ComponentValue::CANVAS_TEXT,
            ComponentValue::CANVAS_GLYPH_RUN,
            ComponentValue::CANVAS_DRAWING,
            ComponentValue::CANVAS_BITMAP,
            ComponentValue::CANVAS_BOX,
        ] {
            let Some(input) = world
                .state
                .entities
                .get(&entity)
                .and_then(|record| record.input(component))
            else {
                continue;
            };
            let key = (entity, component);
            let retained = self.previous.and_then(|view| view.geometry.get(&key));
            let constraint_changed = component == ComponentValue::CANVAS_TEXT
                && self
                    .previous
                    .and_then(|view| view.text_constraints.get(&entity))
                    != Some(&max_width);
            let geometry = if !self.resources_dirty
                && !self.dirty.contains(&entity)
                && !constraint_changed
                && let Some(retained) = retained
            {
                if component == ComponentValue::CANVAS_TEXT && retained.is_some() {
                    self.view.work.reused_texts += 1;
                }
                retained.clone()
            } else {
                let geometry = prepare_constrained_geometry(
                    world,
                    self.context.asset_resources(),
                    CanvasTarget {
                        entity,
                        component,
                        incarnation: input.incarnation,
                    },
                    max_width,
                );
                if component == ComponentValue::CANVAS_TEXT && geometry.is_some() {
                    self.view.work.text_measurements += 1;
                }
                geometry
            };
            if component == ComponentValue::CANVAS_TEXT {
                self.view.text_constraints.insert(entity, max_width);
            }
            if let Some(geometry) = &geometry {
                let extent = match geometry {
                    CanvasGeometry::Glyphs {
                        size,
                        ..
                    }
                    | CanvasGeometry::Bitmap {
                        size,
                        ..
                    }
                    | CanvasGeometry::Box {
                        size,
                        ..
                    } => *size,
                    CanvasGeometry::Drawing {
                        drawing,
                    } => self
                        .context
                        .asset_resources()
                        .get_typed::<crate::services::asset_management::drawing::DrawingAsset>(
                            *drawing,
                        )
                        .map_or([0.0; 2], |drawing| {
                            let bounds = drawing.view_box();
                            [bounds[2] - bounds[0], bounds[3] - bounds[1]]
                        }),
                };
                size = [size[0].max(extent[0]), size[1].max(extent[1])];
            }
            self.view.geometry.insert(key, geometry);
        }
        size
    }

    fn flex(
        &mut self,
        children: &[EntityId],
        constraints: Constraints,
        fill: [f32; 2],
        horizontal: bool,
        depth: usize,
    ) -> [f32; 2] {
        let main_axis = usize::from(!horizontal);
        let cross_axis = 1 - main_axis;
        let max_main = if horizontal {
            constraints.max_w
        } else {
            constraints.max_h
        };
        let max_cross = if horizontal {
            constraints.max_h
        } else {
            constraints.max_w
        };
        let flex_total: f32 = children.iter().map(|child| self.style(*child).flex).sum();
        let mut cursor = 0.0;
        let mut cross = 0.0_f32;
        let mut measured = BTreeMap::new();
        let mut leftover = 0.0;
        for flexible in [false, true] {
            for &child in children {
                let style = self.style(child);
                if (style.flex > 0.0) != flexible {
                    continue;
                }
                let margin = style.margin();
                let (start, end) = if horizontal {
                    (margin[3], margin[1])
                } else {
                    (margin[0], margin[2])
                };
                let main = if flexible {
                    if max_main.is_finite() && flex_total > 0.0 {
                        leftover * style.flex / flex_total
                    } else {
                        0.0
                    }
                } else {
                    max_main - cursor
                };
                let main = (main - start - end).max(0.0);
                let mut child_constraints = if horizontal {
                    Constraints::loose(main, max_cross)
                } else {
                    Constraints::loose(max_cross, main)
                };
                if flexible {
                    if horizontal {
                        child_constraints.min_w = main;
                    } else {
                        child_constraints.min_h = main;
                    }
                }
                let size = self.visit(child, child_constraints, depth + 1);
                measured.insert(child, size);
                cursor += start + size[main_axis] + end;
                cross = cross.max(size[cross_axis]);
            }
            if !flexible {
                leftover = (max_main - cursor).max(0.0);
            }
        }
        let mut position = 0.0;
        for &child in children {
            let size = measured[&child];
            let style = self.style(child);
            let margin = style.margin();
            let (start, end, cross_start, alignment) = if horizontal {
                (margin[3], margin[1], margin[0], style.align_y)
            } else {
                (margin[0], margin[2], margin[3], style.align_x)
            };
            let mut origin = [0.0; 2];
            origin[main_axis] = position + start;
            origin[cross_axis] = cross_start
                + alignment_factor(alignment, -1.0) * (cross - size[cross_axis]).max(0.0);
            self.place(child, origin);
            position += start + size[main_axis] + end;
        }
        let mut size = [0.0; 2];
        size[main_axis] = cursor;
        size[cross_axis] = cross;
        [fill_or_fit(size[0], fill[0]), fill_or_fit(size[1], fill[1])]
    }

    fn stack(
        &mut self,
        children: &[EntityId],
        constraints: Constraints,
        fill: [f32; 2],
        depth: usize,
    ) -> [f32; 2] {
        let mut extent = [0.0_f32; 2];
        let mut measured = Vec::new();
        for &child in children {
            let margin = self.style(child).margin();
            let outer = [margin[3] + margin[1], margin[0] + margin[2]];
            let size = self.visit(
                child,
                Constraints::loose(
                    (constraints.max_w - outer[0]).max(0.0),
                    (constraints.max_h - outer[1]).max(0.0),
                ),
                depth + 1,
            );
            let size = [(size[0] + outer[0]).max(0.0), (size[1] + outer[1]).max(0.0)];
            extent = [extent[0].max(size[0]), extent[1].max(size[1])];
            measured.push((child, size));
        }
        let size = [
            fill_or_fit(extent[0], fill[0]),
            fill_or_fit(extent[1], fill[1]),
        ];
        for (child, child_size) in measured {
            let style = self.style(child);
            self.place(
                child,
                [
                    style.margin_left
                        + alignment_factor(style.align_x, -1.0)
                            * (size[0] - child_size[0]).max(0.0),
                    style.margin_top
                        + alignment_factor(style.align_y, -1.0)
                            * (size[1] - child_size[1]).max(0.0),
                ],
            );
        }
        size
    }

    fn single(
        &mut self,
        children: &[EntityId],
        style: GuiLayout,
        constraints: Constraints,
        fill: [f32; 2],
        depth: usize,
    ) -> [f32; 2] {
        let size = [fill_or_fit(0.0, fill[0]), fill_or_fit(0.0, fill[1])];
        let child_size = children.first().map_or([0.0; 2], |child| {
            let measured = self.visit(*child, constraints, depth + 1);
            if style.kind == 5 {
                self.place(
                    *child,
                    [
                        alignment_factor(style.align_x, 0.0) * (size[0] - measured[0]).max(0.0),
                        alignment_factor(style.align_y, 0.0) * (size[1] - measured[1]).max(0.0),
                    ],
                );
            }
            measured
        });
        for &child in children.iter().skip(1) {
            self.view
                .diagnostics
                .push(GuiEntityLayoutDiagnostic::ExtraChild {
                    entity: child,
                });
            self.view.placements.insert(
                child,
                GuiEntityLayout {
                    origin: [0.0; 2],
                    size: [0.0; 2],
                    content_offset: [0.0; 2],
                    clip: true,
                    available: false,
                },
            );
        }
        if style.kind == 6 {
            [
                if style.width >= 0.0 {
                    style.width
                } else {
                    child_size[0]
                },
                if style.height >= 0.0 {
                    style.height
                } else {
                    child_size[1]
                },
            ]
        } else {
            [
                fill_or_fit(child_size[0], fill[0]),
                fill_or_fit(child_size[1], fill[1]),
            ]
        }
    }
}

/// A scrolling control's stored position and lifetime, read before layout.
struct GuiScrolling {
    component: u16,
    incarnation: u64,
    position: GuiScrollFieldPosition,
    /// The control's evaluated `available` field.
    available: bool,
}

fn optional(value: f32) -> f32 {
    if value < 0.0 {
        f32::INFINITY
    } else {
        value
    }
}

fn alignment_factor(value: f32, default: f32) -> f32 {
    align_factor((value != 2.0).then_some(value), default)
}
