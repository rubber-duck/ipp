//! Shared fixtures for GUI layout and cache tests.
//!
//! Expectations are hand-computed from the fixture font table below and
//! the documented layout rules, never derived from the implementation.
//! Fixture font (units per em 1000, ascender 800, descender -200, gap
//! 200): space advances 300, `A` maps to glyph 1 advancing 600, `V` to
//! glyph 2 advancing 600, `a` to glyph 3 advancing 650, `e` to glyph 5
//! advancing 550. Line height is 1.2 em everywhere.

use super::super::super::test_support::font_source;
use super::*;
use crate::DynamicValue;
use crate::EntityId;
use crate::services::asset_management::font::{FONT_TYPE, FontAsset};
use crate::services::asset_management::{AssetKey, AssetSource};
use crate::systems::surface::SurfaceRenderResource;
use std::collections::BTreeMap;

pub(super) const FONT_SIZE: f32 = 0.1;
/// Logical units per em at the default units factor: 0.1 m/em * 1.0.
pub(super) const EM: f32 = 0.1;
/// Line height in logical units: 1.2 em * 0.1.
pub(super) const LINE: f32 = 0.12;
/// Advance of `A` in logical units: 0.6 em * 0.1.
pub(super) const ADV_A: f32 = 0.06;
pub(super) fn draw_source() -> AssetSource {
    AssetSource {
        kind: FONT_TYPE,
        uri: "test-draw".to_owned(),
        variant: 0,
    }
}
/// Resolver serving one fixture font plus named surface resources.
pub(super) struct TestResolver<'a> {
    pub(super) font: Option<(&'a FontAsset, AssetKey)>,
    pub(super) pending_font: bool,
    pub(super) resources: BTreeMap<String, SurfaceRenderResource>,
}
impl<'a> TestResolver<'a> {
    pub(super) fn with_font(font: &'a FontAsset) -> Self {
        Self {
            font: Some((
                font,
                AssetKey {
                    slot: 3,
                    generation: 1,
                },
            )),
            pending_font: false,
            resources: BTreeMap::new(),
        }
    }

    pub(super) fn with_resource(mut self, uri: &str, key: AssetKey) -> Self {
        self.resources.insert(
            uri.to_owned(),
            SurfaceRenderResource {
                key,
                source: AssetSource {
                    kind: FONT_TYPE,
                    uri: uri.to_owned(),
                    variant: 0,
                },
            },
        );
        self
    }
}
impl GuiResourceResolver for TestResolver<'_> {
    fn text_font(&self, source: &AssetSource) -> GuiFontResolution<'_> {
        if source.uri != "test-font" {
            return GuiFontResolution::Missing;
        }

        match self.font {
            Some((font, key)) => GuiFontResolution::Ready {
                key,
                font,
            },
            None if self.pending_font => GuiFontResolution::Pending {
                key: AssetKey {
                    slot: 3,
                    generation: 1,
                },
            },
            None => GuiFontResolution::Missing,
        }
    }

    fn surface_resource(&self, source: &AssetSource) -> Option<SurfaceRenderResource> {
        self.resources.get(&source.uri).cloned()
    }
}
/// Incremental tree builder over authoritative storage.
pub(super) struct TreeBuilder {
    pub(super) root: GuiRoot,
}
impl TreeBuilder {
    pub(super) fn new() -> Self {
        Self {
            root: GuiRoot::default(),
        }
    }

    pub(super) fn add(
        &mut self,
        parent: Option<GuiNodeId>,
        content: GuiNodeContent,
        style: GuiNodeStyle,
    ) -> GuiNodeId {
        let id = GuiNodeId(self.root.nodes().next_node_id());
        let index = parent
            .and_then(|parent| self.root.nodes().node(parent))
            .map(|node| node.children.len())
            .unwrap_or(0);
        self.root
            .nodes_mut()
            .insert_node(id, parent, index, content.clone())
            .unwrap();
        // Mirror the production insert path: a new control node commits its
        // initial value at revision 1, so evaluation observes effective
        // values exactly like world-driven trees.
        self.root.controls_mut().insert_initial(id, &content);
        self.root.install_node_style(id, &style).unwrap();
        id
    }

    pub(super) fn set_visual(&mut self, id: GuiNodeId, position: [f32; 2], scale: [f32; 2]) {
        let position_name = GuiRoot::property_name(id, "position").unwrap();
        let scale_name = GuiRoot::property_name(id, "scale").unwrap();
        self.root
            .properties
            .set(&position_name, DynamicValue::Vec2(position))
            .unwrap();
        self.root
            .properties
            .set(&scale_name, DynamicValue::Vec2(scale))
            .unwrap();
    }

    pub(super) fn build(self) -> GuiRoot {
        self.root
    }
}
pub(super) fn text_style() -> GuiNodeStyle {
    GuiNodeStyle {
        font_size: FONT_SIZE,
        asset: Some(font_source()),
        ..Default::default()
    }
}
pub(super) fn sized(w: f32, h: f32) -> GuiNodeStyle {
    GuiNodeStyle {
        width: Some(w),
        height: Some(h),
        ..Default::default()
    }
}
pub(super) fn request<'a>(root: &'a GuiRoot, tick: u64) -> GuiLayoutRequest<'a> {
    GuiLayoutRequest {
        root,
        root_incarnation: 7,
        surface_size: [4.0, 2.0],
        units_per_metre: DEFAULT_UNITS_PER_METRE,
        evaluation_tick: tick,
    }
}
pub(super) fn entity() -> EntityId {
    EntityId::from_bits(1)
}
pub(super) fn node_by_id(view: &GuiEvaluatedView, id: GuiNodeId) -> &GuiEvaluatedNode {
    view.nodes.iter().find(|node| node.node == id).unwrap()
}
pub(super) fn assert_rect(actual: [f32; 4], expected: [f32; 4]) {
    for (actual, expected) in actual.iter().zip(expected.iter()) {
        assert!(
            (actual - expected).abs() < 1e-5,
            "rect {actual:?} != expected {expected:?}"
        );
    }
}

/// Hit-test helper keeping the visual assertions independent of node reads.
pub(super) fn view_hit(view: &GuiEvaluatedView, point: [f32; 2]) -> GuiNodeId {
    view.hit_test(point).unwrap().node
}
