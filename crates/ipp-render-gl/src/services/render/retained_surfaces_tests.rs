//! Tests for reusing hashes of unchanged Surface paint.

use std::collections::BTreeSet;

use super::{SurfacePaint, SurfacePaintTracker};
use ipp_core::services::asset_management::{AssetKey, AssetSource, drawing::DRAWING_TYPE};
use ipp_core::systems::surface::{
    SurfacePrimitiveIdentity, SurfacePrimitiveStyle, SurfaceRenderPrimitive, SurfaceRenderResource,
};
use ipp_core::{EntityId, SurfaceItemId, SurfaceRenderItem};

fn drawing(id: u32, x: f32) -> SurfaceRenderPrimitive {
    SurfaceRenderPrimitive::Drawing {
        style: SurfacePrimitiveStyle {
            identity: SurfacePrimitiveIdentity::Authored(SurfaceItemId(id)),
            position: [x, 0.0],
            scale: [1.0, 1.0],
            color: [1.0; 4],
            opacity: 1.0,
            clip: None,
        },
        drawing: SurfaceRenderResource {
            key: AssetKey {
                slot: 1,
                generation: 1,
            },
            source: AssetSource {
                kind: DRAWING_TYPE,
                uri: "fixture://drawing".into(),
                variant: 0,
            },
        },
    }
}

fn item(revision: u64, primitives: Vec<SurfaceRenderPrimitive>) -> SurfaceRenderItem {
    SurfaceRenderItem {
        entity: EntityId::from_bits(7),
        model: [0.0; 16],
        anchor: [0.0; 3],
        clip_size: [1.0, 1.0],
        primitives,
        cache: None,
        paint_revision: revision,
        resource_revision: 0,
        #[cfg(feature = "gui")]
        interaction: false,
    }
}

#[test]
fn paint_is_reusable_after_a_draw_at_the_same_revision_and_identity_order() {
    let mut tracker = SurfacePaintTracker::default();
    let first = item(5, vec![drawing(1, 0.0), drawing(2, 0.5)]);
    assert_eq!(
        tracker.paint(&first),
        SurfacePaint {
            revision: 5,
            reusable: false,
        }
    );

    // Undrawn paint never becomes reusable.
    assert!(!tracker.paint(&first).reusable);
    tracker.drawn(&first, tracker.paint(&first));
    let paint = tracker.paint(&first);
    assert!(paint.reusable);
    assert!(paint.reuses(5));
    assert!(!paint.reuses(4), "hashes from another revision are stale");

    // A new revision needs hashing again.
    let edited = item(6, vec![drawing(1, 0.0), drawing(2, 0.7)]);
    assert!(!tracker.paint(&edited).reusable);
}

#[test]
fn identical_paint_moving_between_identities_is_not_reusable() {
    let mut tracker = SurfacePaintTracker::default();
    let before = item(5, vec![drawing(1, 0.0), drawing(2, 0.5)]);
    tracker.drawn(&before, tracker.paint(&before));

    // Core's revision ignores identities, so a swap keeps the revision.
    let swapped = item(5, vec![drawing(2, 0.0), drawing(1, 0.5)]);
    assert!(!tracker.paint(&swapped).reusable);
    let shorter = item(5, vec![drawing(1, 0.0)]);
    assert!(!tracker.paint(&shorter).reusable);

    tracker.drawn(&swapped, tracker.paint(&swapped));
    assert!(tracker.paint(&swapped).reusable);
}

#[test]
fn unknown_revisions_and_forgotten_surfaces_are_never_reusable() {
    let mut tracker = SurfacePaintTracker::default();
    let unknown = item(0, vec![drawing(1, 0.0)]);
    tracker.drawn(&unknown, tracker.paint(&unknown));
    assert_eq!(tracker.paint(&unknown), SurfacePaint::UNKNOWN);

    let known = item(3, vec![drawing(1, 0.0)]);
    tracker.drawn(&known, tracker.paint(&known));
    assert!(tracker.paint(&known).reusable);
    tracker.retain(&BTreeSet::new());
    assert!(!tracker.paint(&known).reusable);
}
