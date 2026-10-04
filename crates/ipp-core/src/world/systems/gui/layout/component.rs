use crate::ErrorReason;
use crate::components::schema::ComponentLifecycle;
use ipp_schema_derive::SchemaComponent;

/// Constraint layout on one ordinary entity; structure is exclusively core ordered links.
/// Padding reduces measurement constraints and offsets content without adding to
/// intrinsic or explicit outer size, following the existing GUI box rules.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct GuiLayout {
    /// Layout operation: leaf 0, row 1, column 2, stack 3, padding 4, align 5, sized box 6.
    pub kind: u32,
    /// Explicit logical width, or -1 for intrinsic/container sizing.
    pub width: f32,
    /// Explicit logical height, or -1 for intrinsic/container sizing.
    pub height: f32,
    /// Minimum logical width.
    pub min_width: f32,
    /// Minimum logical height.
    pub min_height: f32,
    /// Maximum logical width, or -1 for unbounded.
    pub max_width: f32,
    /// Maximum logical height, or -1 for unbounded.
    pub max_height: f32,
    /// Share of remaining row/column space; zero selects intrinsic measurement.
    pub flex: f32,
    /// Horizontal alignment in -1..=1; 2 selects the container's default.
    pub align_x: f32,
    /// Vertical alignment in -1..=1; 2 selects the container's default.
    pub align_y: f32,
    /// Top inner logical inset.
    pub padding_top: f32,
    /// Right inner logical inset.
    pub padding_right: f32,
    /// Bottom inner logical inset.
    pub padding_bottom: f32,
    /// Left inner logical inset.
    pub padding_left: f32,
    /// Top outer logical margin; negative margins are permitted.
    pub margin_top: f32,
    /// Right outer logical margin; negative margins are permitted.
    pub margin_right: f32,
    /// Bottom outer logical margin; negative margins are permitted.
    pub margin_bottom: f32,
    /// Left outer logical margin; negative margins are permitted.
    pub margin_left: f32,
    /// Clip this entity and its descendants to the settled logical box.
    pub clip: bool,
}

impl Default for GuiLayout {
    fn default() -> Self {
        Self {
            kind: 0,
            width: -1.0,
            height: -1.0,
            min_width: 0.0,
            min_height: 0.0,
            max_width: -1.0,
            max_height: -1.0,
            flex: 0.0,
            align_x: 2.0,
            align_y: 2.0,
            padding_top: 0.0,
            padding_right: 0.0,
            padding_bottom: 0.0,
            padding_left: 0.0,
            margin_top: 0.0,
            margin_right: 0.0,
            margin_bottom: 0.0,
            margin_left: 0.0,
            clip: false,
        }
    }
}

impl GuiLayout {
    pub(super) fn padding(self) -> [f32; 4] {
        [
            self.padding_top,
            self.padding_right,
            self.padding_bottom,
            self.padding_left,
        ]
    }

    pub(super) fn margin(self) -> [f32; 4] {
        [
            self.margin_top,
            self.margin_right,
            self.margin_bottom,
            self.margin_left,
        ]
    }
}

/// Raises an entity out of its parent's layout flow and places it against
/// its parent's evaluated box: an overlay such as a list, menu, popover,
/// tooltip, toast stack or dialog.
///
/// The overlay takes no space among its siblings. It is laid out as its own
/// root, sized by its content, and placed in canvas coordinates beside its
/// parent's box on `side`, aligned along that side by `align`. A top-level
/// overlay is placed inside the canvas instead, against the canvas edge
/// `side` names. Its own `CanvasStyle` translation offsets the placement, so
/// a gap below a trigger or a pointer point is authored there; flipping to
/// the opposite side mirrors the offset along the flipped axis. When the
/// preferred side lacks room and the opposite side has more, the overlay
/// flips; it then shifts along the other axis to stay inside the canvas, and
/// an axis whose content exceeds the room is laid out again limited to it,
/// so the content scrolls.
///
/// The overlay is open while its `GuiBehavior.visible` field is set, which it
/// requires; a closed overlay is not laid out, painted, hit or traversed. It
/// establishes its own priority and clipping scope, without a CanvasStyle.
/// Popup, dialog and notification bands are independent of interaction mode.
/// A nested overlay inherits at least its owner's band. Within the same band,
/// a nested scope follows its owner's complete component layers; later sibling
/// scopes follow the earlier scope and all of its nested overlays. Promotion
/// to a higher band takes priority while preserving logical entity ancestry.
///
/// Its `mode` decides what besides client writes opens and closes it, and how
/// it takes input:
///
/// - manual (a toast stack): only client writes open and close it.
/// - light (a menu, option list or popover): a press outside it and its
///   parent's subtree closes it and is swallowed, and so does focus moving
///   outside both. Its box takes the pointer from lower layers.
/// - modal (a dialog): the pointer, hover, the wheel and keys never reach the
///   lower layers of its canvas, and Tab stays inside it.
/// - hint (a tooltip): the GUI System opens it after a delay while its parent
///   control is hovered or holds visible focus, and closes it after a grace
///   when that ends, when the parent is pressed or on Escape. It is inert to
///   the pointer and never takes focus.
///
/// When a light or modal overlay holding a focusable control opens, focus
/// moves to its first one and closing it returns focus to the control focused
/// before, and Escape closes the topmost overlay of the focused control's
/// canvas that is not manual. The input router documents these rules.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct GuiOverlay {
    /// Side of the parent's box the overlay sits beside: bottom 0, top 1,
    /// right 2, left 3, or centred over it 4. Against the canvas, the edge
    /// it rests on inside the canvas, or centred.
    pub side: u32,
    /// Alignment along the side: start 0, centre 1, end 2, or stretch 3 to
    /// the box's extent. Over the box (side 4) it applies horizontally.
    pub align: u32,
    /// Who opens and closes it besides client writes of its visible field:
    /// manual 0, light 1, modal 2 or hint 3.
    pub mode: u32,
    /// Priority band: popup 1, dialog 2 or notification 3. Structural and
    /// independent of mode; a nested overlay inherits at least its owner's band.
    pub band: u32,
}

impl Default for GuiOverlay {
    fn default() -> Self {
        Self {
            side: 0,
            align: 0,
            mode: Self::MODE_MANUAL,
            band: Self::BAND_POPUP,
        }
    }
}

impl GuiOverlay {
    /// Popups above all ordinary content.
    pub const BAND_POPUP: u32 = 1;

    /// Dialog scopes above popups.
    pub const BAND_DIALOG: u32 = 2;

    /// Notifications above dialogs.
    pub const BAND_NOTIFICATION: u32 = 3;

    /// Largest `side`: centred over the box.
    pub(super) const SIDE_CENTRE: u32 = 4;

    /// `align` that stretches the overlay to the box's extent.
    pub(super) const ALIGN_STRETCH: u32 = 3;

    /// `mode`: only client writes open and close it.
    pub const MODE_MANUAL: u32 = 0;

    /// `mode`: an outside press or focus leaving closes it.
    pub const MODE_LIGHT: u32 = 1;

    /// `mode`: it blocks input to the lower layers of its canvas.
    pub const MODE_MODAL: u32 = 2;

    /// `mode`: hovering or visibly focusing its parent opens it.
    pub const MODE_HINT: u32 = 3;
}

impl ComponentLifecycle for GuiOverlay {
    fn required_components() -> &'static [u16] {
        &[crate::ComponentValue::GUI_BEHAVIOR]
    }

    fn animatable_field(_offset: u32) -> bool {
        false
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if self.side > Self::SIDE_CENTRE
            || self.align > Self::ALIGN_STRETCH
            || self.mode > Self::MODE_HINT
            || !(Self::BAND_POPUP..=Self::BAND_NOTIFICATION).contains(&self.band)
        {
            Err(ErrorReason::InvalidValue)
        } else {
            Ok(())
        }
    }
}

impl ComponentLifecycle for GuiLayout {
    fn validate(&self) -> Result<(), ErrorReason> {
        let optional = [self.width, self.height, self.max_width, self.max_height];
        let lengths = [self.min_width, self.min_height, self.flex];
        let aligns = [self.align_x, self.align_y];
        if self.kind > 6
            || optional
                .iter()
                .any(|value| !value.is_finite() || (*value < 0.0 && *value != -1.0))
            || lengths
                .iter()
                .chain(self.padding().iter())
                .any(|value| !value.is_finite() || *value < 0.0)
            || self.margin().iter().any(|value| !value.is_finite())
            || aligns
                .iter()
                .any(|value| !value.is_finite() || (!(-1.0..=1.0).contains(value) && *value != 2.0))
            || (self.max_width >= 0.0 && self.min_width > self.max_width)
            || (self.max_height >= 0.0 && self.min_height > self.max_height)
        {
            Err(ErrorReason::InvalidValue)
        } else {
            Ok(())
        }
    }
}
