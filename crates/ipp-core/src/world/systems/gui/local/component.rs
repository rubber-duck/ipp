use crate::components::schema::ComponentLifecycle;
use crate::{ComponentValue, ErrorReason};
use ipp_schema_derive::SchemaComponent;
use std::sync::Arc;

/// Inherited local interaction policy, independent of native input ownership.
///
/// Every control requires it. `enabled` and `visible` are inherited by
/// descendants; `focus_scope` bounds traversal in this subtree and `focusable`
/// applies to this entity's own control. The eligibility fields hold this
/// entity's evaluated result, which the GUI System writes after each commit.
/// An entity without this component contributes the defaults to its
/// descendants. A control's skin transition channels live beside its fields,
/// outside the schema.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct GuiBehavior {
    /// Whether this entity and its descendants accept control actions.
    pub enabled: bool,
    /// Whether this entity and its descendants participate in paint and input.
    pub visible: bool,
    /// Whether keyboard traversal is bounded by this subtree.
    pub focus_scope: bool,
    /// Whether this entity's control takes focus. A control that does not is
    /// no Tab stop, a pointer press on it leaves focus where it is and a
    /// focus action on it is refused; it still takes pointer feedback and
    /// publishes its press. Not inherited.
    pub focusable: bool,
    /// Explicit semantic name; an empty value uses the control's label.
    pub semantic_label: Arc<str>,
    /// Evaluated: this entity and all its ancestors are enabled.
    pub effective_enabled: bool,
    /// Evaluated: this entity and all its ancestors are visible.
    pub effective_visible: bool,
    /// Evaluated: the control and its local scope are ready for actions and input.
    pub available: bool,
    /// Private transition channels, excluded from authoring, comparison and
    /// persistence.
    #[schema(ignore)]
    pub motion: crate::systems::gui::motion::GuiMotionRuntime,
}

impl Default for GuiBehavior {
    fn default() -> Self {
        Self {
            enabled: true,
            visible: true,
            focus_scope: false,
            focusable: true,
            semantic_label: Arc::default(),
            effective_enabled: true,
            effective_visible: true,
            available: true,
            motion: Default::default(),
        }
    }
}

impl ComponentLifecycle for GuiBehavior {
    fn validate(&self) -> Result<(), ErrorReason> {
        bounded_text(&self.semantic_label)
    }
}

/// A momentary control, and the item of composite rows, tabs, options and
/// menus. Pressing it does not create a committed value.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiButton {
    /// Authored visible and default semantic label.
    pub label: Arc<str>,
    /// Whether the button shows as selected, painted through the checked part
    /// variants. Clients write it; pressing the button does not change it.
    pub selected: bool,
}

impl ComponentLifecycle for GuiButton {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        bounded_text(&self.label)
    }
}

/// A boolean control whose committed value is an ordinary field.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiCheckbox {
    /// Authored visible and default semantic label.
    pub label: Arc<str>,
    /// Committed value, written by clients and by the GUI System.
    pub checked: bool,
}

impl ComponentLifecycle for GuiCheckbox {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        bounded_text(&self.label)
    }
}

/// `GuiSlider.axis`: a dial.
pub const GUI_SLIDER_DIAL: u32 = 2;

/// The one scalar control: a numeric value that is an ordinary field,
/// presented along a horizontal or vertical rail or as a dial.
///
/// Arrow keys and the wheel over the focused slider move the value by
/// `step`, or by `fine_step` while Shift is held; Home and End move it to the
/// bounds. The fill runs between `origin` and the value, so an origin inside
/// the range makes a bipolar slider. A dial paints the range over a
/// 270-degree arc and changes its value by relative vertical drags; see
/// [the slider geometry](super::slider).
///
/// A `range` holds a second ordered value, `upper`, with a thumb of its own:
/// `value` is the lower value and `min <= value <= upper <= max` holds for
/// every write the runtime makes. A write that would put `value` above
/// `upper` is refused, so a client changing both writes them together (a
/// component write of both fields). Each thumb is a focus part of the
/// control, the lower part 0 and the upper part 1: Tab reaches them in that
/// order, and keys, the wheel and drags move the focused or grabbed thumb up
/// to the other one. The fill runs between the thumbs; `origin` is unused.
/// A dial holds one value, so a range is a rail.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct GuiSlider {
    /// Inclusive lower bound for accepted commits.
    pub min: f32,
    /// Inclusive upper bound for accepted commits.
    pub max: f32,
    /// Step of the arrow keys and the wheel, to whose grid from `min` they
    /// and drags snap; zero means continuous input, whose keys move a
    /// hundredth of the range.
    pub step: f32,
    /// Step of the arrow keys and the wheel while Shift is held, to whose
    /// grid they snap; zero uses `step`.
    pub fine_step: f32,
    /// Committed value, written by clients and by the GUI System.
    pub value: f32,
    /// The value the fill runs from to the thumb, clamped into the range: an
    /// origin of zero between a negative minimum and a positive maximum fills
    /// only between zero and the thumb. The default, the lowest `f32`, fills
    /// from the minimum.
    pub origin: f32,
    /// Presentation: a horizontal rail 0, with the minimum at the left; a
    /// vertical rail 1, with the minimum at the bottom; a dial 2, with the
    /// minimum at the lower left, dragged along the vertical axis.
    pub axis: u32,
    /// Whether the slider is a range of `value` to `upper`, with two thumbs.
    pub range: bool,
    /// A range's upper value, written by clients and by the GUI System; never
    /// below `value`. A slider that is not a range ignores it.
    pub upper: f32,
}

impl GuiSlider {
    /// The rail's axis as a coordinate index: 0 for x, 1 for y.
    pub(in crate::world::systems) fn rail_axis(&self) -> usize {
        usize::from(self.axis == 1)
    }

    /// The slider's thumbs, each a focus part: two for a range, else one.
    pub(in crate::world::systems) fn thumbs(&self) -> u32 {
        if self.range {
            2
        } else {
            1
        }
    }

    /// The value of thumb `part`: a range's `upper` for part 1, else `value`.
    pub(in crate::world::systems) fn thumb_value(&self, part: u32) -> f32 {
        if self.range && part == 1 {
            self.upper
        } else {
            self.value
        }
    }

    /// `requested` clamped to where thumb `part` may go: the range, and on a
    /// range up to the other thumb, which wins where the two disagree so the
    /// result never inverts the values.
    pub(in crate::world::systems) fn thumb_clamp(&self, part: u32, requested: f32) -> f32 {
        let within = requested.max(self.min).min(self.max);
        match (self.range, part) {
            (true, 0) => within.min(self.upper),
            (true, _) => within.max(self.value),
            (false, _) => within,
        }
    }

    /// The field offset of thumb `part`'s value.
    pub(in crate::world::systems) fn thumb_field(&self, part: u32) -> usize {
        if self.range && part == 1 {
            std::mem::offset_of!(GuiSlider, upper)
        } else {
            std::mem::offset_of!(GuiSlider, value)
        }
    }

    /// Whether the slider is presented as a dial.
    pub(in crate::world::systems) fn is_dial(&self) -> bool {
        self.axis == GUI_SLIDER_DIAL
    }

    /// Where `value` lies in the range, from 0 at the minimum to 1 at the
    /// maximum; an empty range places every value at the minimum.
    pub(in crate::world::systems) fn fraction(&self, value: f32) -> f32 {
        if self.max > self.min {
            ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }
}

impl Default for GuiSlider {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 1.0,
            step: 0.0,
            fine_step: 0.0,
            value: 0.0,
            origin: f32::MIN,
            axis: 0,
            range: false,
            upper: 1.0,
        }
    }
}

impl ComponentLifecycle for GuiSlider {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if [
            self.min,
            self.max,
            self.step,
            self.fine_step,
            self.value,
            self.origin,
            self.upper,
        ]
        .into_iter()
        .all(f32::is_finite)
            && self.min <= self.max
            && self.step >= 0.0
            && self.fine_step >= 0.0
            && self.axis <= GUI_SLIDER_DIAL
            && (!self.range || (self.value <= self.upper && !self.is_dial()))
        {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// The colour control: one colour as hue, saturation, value and alpha, each
/// an ordinary field in `0..=1`.
///
/// Hue, saturation and value are the HSV model on sRGB-encoded values, the
/// colour space hex and RGB readouts display; a client converts the colour to
/// its own working space at its boundary. Holding the colour as HSV keeps its
/// hue at zero saturation or value, so the hue rail never jumps when the
/// field's marker reaches its white or black edge. Hue runs in turns from red
/// at 0 through yellow, green, cyan, blue and magenta to red again at 1, and
/// does not wrap. Alpha is linear coverage, as the runtime composites it.
///
/// The control paints a saturation-value field, a hue rail and, with
/// `alpha_rail`, an alpha rail over a checker, each a focus part in that
/// order with its own arrows, drags, pointer feedback and focus ring, and a
/// swatch of the colour, all from these fields in the same frame; see [the
/// colour geometry](super::color). Without the alpha rail the control edits
/// an opaque colour: it keeps `alpha` as written and paints its swatch
/// opaque.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct GuiColor {
    /// Hue in turns, written by clients and by the GUI System.
    pub hue: f32,
    /// Saturation, written by clients and by the GUI System.
    pub saturation: f32,
    /// Value, written by clients and by the GUI System.
    pub value: f32,
    /// Alpha, written by clients and by the GUI System.
    pub alpha: f32,
    /// Whether the control shows its alpha rail, a third focus part.
    pub alpha_rail: bool,
}

impl GuiColor {
    /// The colour's channels in field order: hue, saturation, value, alpha.
    pub(in crate::world::systems) fn channels(&self) -> [f32; 4] {
        [self.hue, self.saturation, self.value, self.alpha]
    }

    /// The control's focus parts: the field, the hue rail and, when shown,
    /// the alpha rail.
    pub(in crate::world::systems) fn parts(&self) -> u32 {
        2 + u32::from(self.alpha_rail)
    }

    /// The field offset of channel `channel` in field order.
    pub(in crate::world::systems) fn channel_field(channel: usize) -> usize {
        match channel {
            0 => std::mem::offset_of!(GuiColor, hue),
            1 => std::mem::offset_of!(GuiColor, saturation),
            2 => std::mem::offset_of!(GuiColor, value),
            _ => std::mem::offset_of!(GuiColor, alpha),
        }
    }
}

impl Default for GuiColor {
    /// Opaque white without the alpha rail.
    fn default() -> Self {
        Self {
            hue: 0.0,
            saturation: 0.0,
            value: 1.0,
            alpha: 1.0,
            alpha_rail: false,
        }
    }
}

impl ComponentLifecycle for GuiColor {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        if self
            .channels()
            .into_iter()
            .all(|channel| (0.0..=1.0).contains(&channel))
        {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// A single-line text control whose committed text is an ordinary field.
///
/// A `numeric` input holds a number instead: its committed `value`, which it
/// shows formatted with `precision` decimals, is distinct from the text being
/// edited, which is GUI System state while the input holds focus. Enter or
/// blur commits the edit, clamped to `min..=max`; text that does not parse
/// leaves the number and is reported, and Escape discards the edit. Up and
/// Down step the number by `step`, or `fine_step` with Shift, and with
/// `step_parts` its decrement and increment parts at the field's ends step it
/// on press and repeat while held. A numeric input ignores `text` and
/// `placeholder`; see [the number rules](super::number).
#[repr(C)]
#[derive(Clone, Debug, PartialEq, SchemaComponent)]
pub struct GuiTextInput {
    /// Text painted when the committed value is empty.
    pub placeholder: Arc<str>,
    /// Committed single-line text, written by clients and by the GUI System.
    pub text: Arc<str>,
    /// Whether the input holds the number `value` rather than `text`.
    pub numeric: bool,
    /// A numeric input's committed number, written by clients and by the GUI
    /// System.
    pub value: f32,
    /// Inclusive lower bound the GUI System clamps commits and steps to; the
    /// default, the lowest `f32`, leaves the number unbounded below.
    pub min: f32,
    /// Inclusive upper bound the GUI System clamps commits and steps to; the
    /// default, the largest `f32`, leaves the number unbounded above.
    pub max: f32,
    /// Increment of Up, Down and the step parts; zero disables stepping.
    pub step: f32,
    /// Increment of Up and Down while Shift is held; zero uses `step`.
    pub fine_step: f32,
    /// Decimals the number shows with, at most [`MAX_GUI_NUMBER_PRECISION`].
    pub precision: u32,
    /// Whether a numeric input shows its decrement and increment parts.
    pub step_parts: bool,
}

/// Most decimals a numeric text input shows.
pub const MAX_GUI_NUMBER_PRECISION: u32 = 9;

impl Default for GuiTextInput {
    fn default() -> Self {
        Self {
            placeholder: Arc::default(),
            text: Arc::default(),
            numeric: false,
            value: 0.0,
            min: f32::MIN,
            max: f32::MAX,
            step: 1.0,
            fine_step: 0.0,
            precision: 0,
            step_parts: false,
        }
    }
}

impl ComponentLifecycle for GuiTextInput {
    fn required_components() -> &'static [u16] {
        &CONTROL_REQUIREMENTS
    }

    fn validate(&self) -> Result<(), ErrorReason> {
        bounded_text(&self.placeholder)?;
        single_line_text(&self.text)?;
        if [self.value, self.min, self.max, self.step, self.fine_step]
            .into_iter()
            .all(f32::is_finite)
            && self.min <= self.max
            && self.step >= 0.0
            && self.fine_step >= 0.0
            && self.precision <= MAX_GUI_NUMBER_PRECISION
        {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// Makes the controls below this entity its items in tree order, down to but
/// not into a nested group or an item, whose own controls are parts of it;
/// scroll views and virtual lists are not items. Arrow keys along `axis`,
/// Home and End move among the eligible items and stop at the ends. Items
/// that take focus make the group one Tab stop, entered at its selected item
/// or its first, and arrows move focus. When no item takes focus the group
/// has an active item instead, GUI System state that paints as hovered:
/// pointer hover moves it, and the keys of the focused control move and
/// activate it while the group lies in the topmost open overlay of that
/// control's canvas.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, SchemaComponent)]
pub struct GuiGroup {
    /// Arrow keys that move among the items: horizontal 0 (Left and Right),
    /// vertical 1 (Up and Down), both 2.
    pub axis: u32,
    /// Selection kept in the Button items' `selected` fields: none 0; single
    /// 1, where activating an item or writing its `selected` true clears the
    /// other items' `selected`; follow 2, which also selects the item arrow
    /// keys move to.
    pub selection: u32,
}

impl Default for GuiGroup {
    fn default() -> Self {
        Self {
            axis: GUI_GROUP_VERTICAL,
            selection: GUI_GROUP_SELECT_NONE,
        }
    }
}

impl ComponentLifecycle for GuiGroup {
    fn validate(&self) -> Result<(), ErrorReason> {
        if self.axis <= GUI_GROUP_BOTH && self.selection <= GUI_GROUP_SELECT_FOLLOW {
            Ok(())
        } else {
            Err(ErrorReason::InvalidValue)
        }
    }
}

/// `GuiGroup.axis`: Left and Right move among the items.
pub const GUI_GROUP_HORIZONTAL: u32 = 0;
/// `GuiGroup.axis`: Up and Down move among the items.
pub const GUI_GROUP_VERTICAL: u32 = 1;
/// `GuiGroup.axis`: every arrow moves among the items.
pub const GUI_GROUP_BOTH: u32 = 2;
/// `GuiGroup.selection`: the group writes no `selected` field.
pub const GUI_GROUP_SELECT_NONE: u32 = 0;
/// `GuiGroup.selection`: one item selected, by activation or a write.
pub const GUI_GROUP_SELECT_SINGLE: u32 = 1;
/// `GuiGroup.selection`: as single, and arrow movement selects too.
pub const GUI_GROUP_SELECT_FOLLOW: u32 = 2;

/// Maximum UTF-8 bytes in one authored, committed, provisional or restored
/// GUI text value.
pub const MAX_GUI_TEXT_BYTES: usize = 65_536;

/// Components every control requires; missing ones are inserted with defaults.
pub(in crate::world::systems::gui) const CONTROL_REQUIREMENTS: [u16; 2] =
    [ComponentValue::GUI_BEHAVIOR, ComponentValue::CANVAS_BOUNDS];

pub(super) const CONTROL_COMPONENTS: [u16; 7] = [
    ComponentValue::GUI_BUTTON,
    ComponentValue::GUI_CHECKBOX,
    ComponentValue::GUI_SLIDER,
    ComponentValue::GUI_TEXT_INPUT,
    ComponentValue::GUI_SCROLL_VIEW,
    ComponentValue::GUI_VIRTUAL_LIST,
    ComponentValue::GUI_COLOR,
];

pub(super) fn bounded_text(text: &str) -> Result<(), ErrorReason> {
    if text.len() <= MAX_GUI_TEXT_BYTES {
        Ok(())
    } else {
        Err(ErrorReason::InvalidValue)
    }
}

pub(super) fn single_line_text(text: &str) -> Result<(), ErrorReason> {
    bounded_text(text)?;
    if text.contains(['\r', '\n']) {
        Err(ErrorReason::InvalidValue)
    } else {
        Ok(())
    }
}
