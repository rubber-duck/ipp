//! Built-in skin looks: named tables of theme part rows in the `GuiTheme`
//! vocabulary.
//!
//! Every control kind has a default look ([`default_look`]), and the slider
//! presented as a dial has its own ([`control_look`]): a built-in theme
//! that is the last link of part resolution: a property resolves from the skin's
//! override row, then the theme's rows from most to least specific, then the
//! default look's rows in the same order. A theme that sets only some properties
//! therefore sits on the default look rather than on neutral values. Named looks
//! that are not a kind's default, such as the switch for a checkbox and the
//! amber, secondary and docked buttons, are tables of the same kind: the
//! generated contract exports every look as data with its `em`, and a client
//! creates an ordinary theme entity from one. A look used as a theme still sits
//! on its control's default look, so it states every property where it differs,
//! including the ones it switches off.
//!
//! # Design language
//!
//! The looks speak the skin lab's design language (`tests/gui/skin-lab/README.md`,
//! "Design language"), written down here as tokens that every control table
//! combines, so changing a token changes every control that uses it.
//!
//! - **Colour**, one per role: `surface` inside controls and for content on a
//!   solid lit fill, `accent` for every lit line, label, solid lit fill and
//!   glow, `text` for content, `neutral` for idle lines, unlit fills and
//!   everything disabled, `line` for quiet lines and rail outlines, `selection`
//!   for selected text and `amber` for the variant that replaces the accent and
//!   the idle line. Rails fill with the quiet line at 15%.
//! - **Shape**: zero radius and one cut pattern, paired on the top-left and
//!   bottom-right corners, the same in every state: the frame cut on
//!   full-size controls and content frames, the part cut on small controls,
//!   parts and secondary buttons, none on a button docked on a container's
//!   lines.
//! - **Lines**: one idle and one lit weight.
//! - **Glow**: one shape, at full strength for focus and half for hover and
//!   press, reaching a frame's distance from control frames and half that from
//!   moving parts.
//! - **States**, identical on every interactive part: idle is the idle line
//!   round the surface, with value marks lit at rest; hover the lit line with
//!   the half glow; pressed the lit fill under the hover edge with content in
//!   the surface colour; focus the lit line with the full glow on the control's
//!   own border; checked, on or selected the lit fill with content in the
//!   surface colour, kept under hover, press and focus; disabled draws
//!   whatever would be lit in `neutral` and keeps geometry and value.
//! - **Hierarchy**: primary buttons take the frame cut and a lit label,
//!   secondary buttons the part cut and a text label, at any size.
//! - **Motion**: hover fades in over 80 ms and out over 120 ms; a press is
//!   immediate and its release fades over 100 ms; disable is immediate both
//!   ways, and so are focus and a check mark; a checked fill or a selection
//!   fades over 100 ms, and the switch block travels in 160 ms with an
//!   ease-out cubic. Each look's `motion` rows state these in the
//!   `GuiThemeMotion` vocabulary, so a theme made from a look moves as the
//!   look does.
//!
//! Three exceptions keep their reasons: a scroll track points its ends, as the
//! one rail drawn inside a frame along its edge would otherwise read as a
//! second box; the slider thumb is an outline at rest, as a solid thumb would
//! merge into the value bar it ends; and a colour control's field, rails and
//! swatch are uncut, as a cut would remove the extreme colours at the field's
//! corners. Circles appear only where the circle is the function, such as the
//! dial's rings inside its cut housing and the colour field's marker, which
//! marks a point equally in every direction.
//!
//! Colours are linear RGBA from sRGB values. Lengths are logical units at the
//! language's body type size, the looks' `em`; like a theme with an `em`, a
//! look draws its lengths `font_size / em` times larger for a control with an
//! inherited font of `font_size`, so it keeps its proportions to the text in a
//! World of any unit scale. Looks for composite entities that are not controls
//! (container frames, separators, grid rows) can join [`gui_skin_looks`] as
//! further named tables; only control kinds have defaults.
//!
//! The language's values are also exported as named tokens
//! ([`gui_skin_tokens`]): every role colour, line weight, cut, glow, size and
//! type size, once, including the sizes and roles that only client
//! compositions use (the page, the error status, container corner accents,
//! docked buttons, rows, insets and the type scale). Clients composing their
//! own parts read them from the generated contract instead of keeping a copy.

use super::GuiPaintPart;
use super::measurement::{CONTROL_HEIGHT, DIAL_SIDE, SMALL_HEIGHT, TEXT_INPUT_INSET};
use super::part_style::GuiSkinState;
use super::parts::{GuiPartId, GuiPartVariant, GuiPrimitivePart};
use crate::systems::gui::layout::scroll_bars::GUI_SCROLL_BAR_EMS;
use crate::systems::gui::local::GuiControlKind;
use crate::systems::gui::local::controls::color::COLOR_MARKER_EMS;
use crate::systems::gui::local::controls::identity::GuiControl;
use crate::systems::gui::local::controls::slider::{DIAL_INSET_EMS, DIAL_SWEEP};
use crate::systems::gui::motion::GuiMotionPart;
use crate::world::WorldSimulationState;
use std::sync::LazyLock;

/// One named table of theme part rows and their transition timing.
#[derive(Clone, Debug, PartialEq)]
pub struct GuiSkinLook {
    /// Contract name of the look.
    pub name: &'static str,
    /// Font size the rows' lengths are drawn at, as a theme's `em`.
    pub em: f32,
    /// Part rows in the `GuiTheme.parts` vocabulary, one per part identity.
    pub parts: Vec<GuiPaintPart>,
    /// Timing rows in the `GuiThemeMotion.parts` vocabulary, one per part
    /// identity.
    pub motion: Vec<GuiMotionPart>,
}

impl GuiSkinLook {
    /// The row of one part identity, if the look styles it.
    pub(in crate::world::systems::gui) fn row(&self, identity: GuiPartId) -> Option<&GuiPaintPart> {
        let index = identity.index()?;
        self.parts.iter().find(|row| row.part == index)
    }
}

/// Every built-in look in contract order: the control defaults, then the
/// named variants.
pub fn gui_skin_looks() -> &'static [GuiSkinLook] {
    &LOOKS
}

/// The default look of `kind`: the look its controls paint under every
/// theme, except a slider presented as a dial ([`control_look`]).
pub(in crate::world::systems::gui) fn default_look(kind: GuiControlKind) -> &'static GuiSkinLook {
    let name = match kind {
        GuiControlKind::Button => BUTTON,
        GuiControlKind::Checkbox => CHECKBOX,
        GuiControlKind::Slider => SLIDER,
        GuiControlKind::TextInput => TEXT_INPUT,
        GuiControlKind::ScrollView | GuiControlKind::VirtualList => SCROLL,
        GuiControlKind::Color => COLOR,
    };
    named(name)
}

/// The look `control` paints under every theme: its kind's default look, or
/// the dial's for a slider presented as a dial.
pub(in crate::world::systems::gui) fn control_look(
    world: &WorldSimulationState,
    control: GuiControl,
) -> &'static GuiSkinLook {
    let dial = control.kind == GuiControlKind::Slider
        && world
            .components
            .gui_slider(control.target.entity.index() as usize)
            .is_some_and(|slider| slider.is_dial());
    if dial {
        named(DIAL)
    } else {
        default_look(control.kind)
    }
}

fn named(name: &str) -> &'static GuiSkinLook {
    LOOKS
        .iter()
        .find(|look| look.name == name)
        .expect("every control presentation has a default look")
}

const BUTTON: &str = "button";
const CHECKBOX: &str = "checkbox";
const SLIDER: &str = "slider";
const DIAL: &str = "dial";
const TEXT_INPUT: &str = "textInput";
const SCROLL: &str = "scroll";
const COLOR: &str = "color";
const SWITCH: &str = "switch";
const AMBER: &str = "amber";
const SECONDARY: &str = "secondary";
const SECONDARY_AMBER: &str = "secondaryAmber";
const DOCKED: &str = "docked";

/// The language's body type size: field text, panel content and primary
/// button labels. Every look's lengths are drawn at it.
pub(in crate::world::systems::gui) const GUI_LOOK_EM: f32 = 16.0;

static LOOKS: LazyLock<Vec<GuiSkinLook>> = LazyLock::new(|| {
    let palette = Palette::new();
    let look = |name, parts, motion| GuiSkinLook {
        name,
        em: GUI_LOOK_EM,
        parts,
        motion,
    };
    let cyan = palette.cyan();
    let amber = palette.amber();
    vec![
        look(
            BUTTON,
            button(&palette, &cyan, cut(CUT), cyan.lit),
            button_motion(),
        ),
        look(CHECKBOX, checkbox(&palette, &cyan), checkbox_motion()),
        look(SLIDER, slider(&palette, &cyan), slider_motion()),
        look(DIAL, dial(&palette, &cyan), dial_motion()),
        look(TEXT_INPUT, text_input(&palette, &cyan), text_input_motion()),
        look(SCROLL, scroll(&palette, &cyan), scroll_motion()),
        look(COLOR, color(&palette, &cyan), color_motion()),
        look(SWITCH, switch(&palette, &cyan), switch_motion()),
        look(
            AMBER,
            button(&palette, &amber, cut(CUT), amber.lit),
            button_motion(),
        ),
        look(
            SECONDARY,
            button(&palette, &cyan, cut(PART_CUT), palette.text),
            button_motion(),
        ),
        look(
            SECONDARY_AMBER,
            button(&palette, &amber, cut(PART_CUT), palette.text),
            button_motion(),
        ),
        look(
            DOCKED,
            button(&palette, &cyan, SQUARE, palette.text),
            button_motion(),
        ),
    ]
});

/// One named value of the design language.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiSkinToken {
    /// Contract name of the token.
    pub name: &'static str,
    /// Its value.
    pub value: GuiSkinTokenValue,
}

/// A token's value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GuiSkinTokenValue {
    /// A length in logical units at the looks' `em`, or, where the name says
    /// so (glow intensities and falloff, a duration in Host seconds, an easing
    /// index), a plain number.
    Number(f32),
    /// Linear RGBA.
    Color([f32; 4]),
}

impl GuiSkinTokenValue {
    /// The value's `f32` lanes: one for a number, four for a colour.
    pub fn lanes(&self) -> &[f32] {
        match self {
            Self::Number(value) => std::slice::from_ref(value),
            Self::Color(value) => value,
        }
    }
}

/// Every token of the design language in contract order: `em`, the role
/// colours, then lines, cuts, corner accents, glow, sizes, type and motion.
pub fn gui_skin_tokens() -> &'static [GuiSkinToken] {
    &TOKENS
}

static TOKENS: LazyLock<Vec<GuiSkinToken>> = LazyLock::new(|| {
    use GuiSkinTokenValue::{Color, Number};

    let palette = Palette::new();
    let em = GUI_LOOK_EM;
    [
        ("em", Number(em)),
        ("page", Color(palette.page)),
        ("surface", Color(palette.surface)),
        ("accent", Color(palette.accent)),
        ("text", Color(palette.text)),
        ("neutral", Color(palette.neutral)),
        ("line", Color(palette.line)),
        ("selection", Color(palette.selection)),
        ("amber", Color(palette.amber)),
        ("error", Color(palette.error)),
        ("railFill", Color(palette.rail_fill)),
        ("rowTint", Color(palette.row_tint)),
        ("lineWidth", Number(LINE)),
        ("litLineWidth", Number(LIT_LINE)),
        ("cut", Number(CUT)),
        ("partCut", Number(PART_CUT)),
        ("cornerAccent", Number(CORNER_ACCENT)),
        ("cornerAccentWidth", Number(CORNER_ACCENT_WIDTH)),
        ("focusGlowIntensity", Number(FOCUS_GLOW)),
        ("hoverGlowIntensity", Number(HOVER_GLOW)),
        ("glowFalloff", Number(GLOW_FALLOFF)),
        ("frameGlowReach", Number(FRAME_GLOW)),
        ("partGlowReach", Number(PART_GLOW)),
        ("checker", Number(CHECKER)),
        ("controlHeight", Number(CONTROL_HEIGHT * em)),
        ("smallHeight", Number(SMALL_HEIGHT * em)),
        ("dial", Number(DIAL_SIDE * em)),
        ("dockedHeight", Number(DOCKED_HEIGHT)),
        ("dockedWidth", Number(DOCKED_WIDTH)),
        ("bar", Number(BAR)),
        ("inset", Number(TEXT_INPUT_INSET * em)),
        ("row", Number(ROW)),
        ("denseRow", Number(DENSE_ROW)),
        ("selectionGutter", Number(SELECTION_GUTTER)),
        ("textSmall", Number(TEXT_SMALL)),
        ("textBody", Number(em)),
        ("textDisplay", Number(TEXT_DISPLAY)),
        ("icon", Number(ICON)),
        ("hoverInSeconds", Number(HOVER_IN)),
        ("hoverOutSeconds", Number(HOVER_OUT)),
        ("releaseSeconds", Number(RELEASE)),
        ("fillSeconds", Number(FILL)),
        ("switchSeconds", Number(SWITCH_TRAVEL)),
        ("switchEasing", Number(EASE_OUT_CUBIC as f32)),
    ]
    .into_iter()
    .map(|(name, value)| GuiSkinToken {
        name,
        value,
    })
    .collect()
});

/// Linear RGBA of an sRGB `0xrrggbb` value.
fn srgb(hex: u32) -> [f32; 4] {
    let channel = |shift: u32| {
        let value = f64::from((hex >> shift) & 0xff) / 255.0;
        let linear = if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        };
        linear as f32
    };
    [channel(16), channel(8), channel(0), 1.0]
}

/// `color` at `alpha`: a tint of a role colour over whatever lies beneath.
fn translucent(color: [f32; 4], alpha: f32) -> [f32; 4] {
    [color[0], color[1], color[2], alpha]
}

/// One colour per role.
struct Palette {
    /// The page and every container's fill: panels, sections, title bars.
    page: [f32; 4],
    /// Control interiors, darker than the page, and content on a solid lit
    /// fill: a pressed label, the check mark.
    surface: [f32; 4],
    /// The lit colour: lit lines, titles and labels, solid lit fills and every
    /// glow; also the information and success status.
    accent: [f32; 4],
    /// Content text: field values, list rows, readouts and secondary button
    /// labels.
    text: [f32; 4],
    /// The unlit colour: idle lines and panel divisions, unlit fills,
    /// secondary text, everything disabled and the inactive status.
    neutral: [f32; 4],
    /// Quiet lines: what repeats between data (row separators, grid lines)
    /// and rail outlines.
    line: [f32; 4],
    /// Text selection, its own blue so selected text stays legible.
    selection: [f32; 4],
    /// The amber variant, replacing the accent and the idle line, and the
    /// warning status.
    amber: [f32; 4],
    /// The error status: magenta, the sheets' one hue beside cyan and amber,
    /// so an error never reads as a warning.
    error: [f32; 4],
    /// Rail interiors: the quiet line at 15%.
    rail_fill: [f32; 4],
    /// The row tint: the accent at 4%, behind a selected row with its accent
    /// bar and behind the active option of a list or menu.
    row_tint: [f32; 4],
}

impl Palette {
    fn new() -> Self {
        let line = srgb(0x355c70);
        let accent = srgb(0x00f4fb);
        Self {
            page: srgb(0x011722),
            surface: srgb(0x00131c),
            accent,
            text: srgb(0xe5f5f7),
            neutral: srgb(0x90b0c4),
            line,
            selection: srgb(0x0b6fc0),
            amber: srgb(0xfbdc6c),
            error: srgb(0xf4449f),
            rail_fill: translucent(line, 0.15),
            row_tint: translucent(accent, 0.04),
        }
    }

    /// The ordinary lit and idle colours.
    fn cyan(&self) -> Variant {
        Variant {
            lit: self.accent,
            idle: self.neutral,
        }
    }

    /// The amber variant: amber for both.
    fn amber(&self) -> Variant {
        Variant {
            lit: self.amber,
            idle: self.amber,
        }
    }
}

/// The two colours a variant replaces in every row.
struct Variant {
    /// Lit lines, labels, fills and glow.
    lit: [f32; 4],
    /// The idle line.
    idle: [f32; 4],
}

/// Width of every idle line.
const LINE: f32 = 1.25;

/// Width of every lit line: hover, press and focus edges.
const LIT_LINE: f32 = 1.5;

/// The frame cut: full-size controls and content frames.
const CUT: f32 = 8.0;

/// The part cut: small controls, parts and secondary buttons.
const PART_CUT: f32 = 4.0;

/// No cut: a button docked on a container's lines takes its square corners.
const SQUARE: [f32; 4] = [0.0; 4];

/// Width of a default scroll bar, whose track points both ends over half of it.
const BAR: f32 = GUI_SCROLL_BAR_EMS * GUI_LOOK_EM;

/// Peak glow at the contour: focus at full strength, hover and press at half,
/// so the keyboard target stays the brightest edge.
const FOCUS_GLOW: f32 = 0.04;
const HOVER_GLOW: f32 = FOCUS_GLOW / 2.0;

/// Glow falloff across its reach.
const GLOW_FALLOFF: f32 = 2.5;

/// Reach of a control frame's glow, outward and inward from its contour.
const FRAME_GLOW: f32 = 16.0;

/// Reach of a moving part's glow: half the frame glow, which would bloom
/// around a part only one or two bars across.
const PART_GLOW: f32 = FRAME_GLOW / 2.0;

/// Cell side of the checker beneath translucent colour: the colour control's
/// alpha rail and swatch.
const CHECKER: f32 = 8.0;

/// Container corner accents: within the frame cut's length of every corner,
/// along both edges, the idle line doubles.
const CORNER_ACCENT: f32 = CUT;
const CORNER_ACCENT_WIDTH: f32 = 2.0 * LINE;

/// Buttons docked in a header strip or title bar, and the switch block.
const DOCKED_HEIGHT: f32 = 24.0;
const DOCKED_WIDTH: f32 = 32.0;

/// Rows of body text (data grid, tree, menu and option rows), and dense rows
/// of small text (logs).
const ROW: f32 = 36.0;
const DENSE_ROW: f32 = 24.0;

/// Width of a selected row's accent bar left of its first line, which every
/// row keeps before its first column.
const SELECTION_GUTTER: f32 = 4.0;

/// The type scale beside the body size: small text (dense rows, readouts,
/// grid headers, secondary text button labels), display text (large value
/// readouts) and icon glyphs.
const TEXT_SMALL: f32 = 13.0;
const TEXT_DISPLAY: f32 = 24.0;
const ICON: f32 = 24.0;

/// The one cut pattern: `size` on the paired top-left and bottom-right corners.
fn cut(size: f32) -> [f32; 4] {
    [size, 0.0, size, 0.0]
}

/// A scroll track's pointed ends: half its `width` cut on every corner.
fn pointed_ends(width: f32) -> [f32; 4] {
    [width / 2.0; 4]
}

fn base(part: GuiPrimitivePart) -> GuiPaintPart {
    keyed(GuiPartId::base(part))
}

fn state(part: GuiPrimitivePart, state: GuiSkinState) -> GuiPaintPart {
    keyed(GuiPartId::state(part, state))
}

fn qualified(part: GuiPrimitivePart, state: GuiSkinState, variant: GuiPartVariant) -> GuiPaintPart {
    keyed(GuiPartId::variant(part, state, variant))
}

fn keyed(identity: GuiPartId) -> GuiPaintPart {
    GuiPaintPart::keyed(identity).expect("compiled part identity")
}

/// Every interaction state, in resolution order of specificity.
const STATES: [GuiSkinState; 4] = [
    GuiSkinState::Idle,
    GuiSkinState::Hovered,
    GuiSkinState::Pressed,
    GuiSkinState::Disabled,
];

/// The glow of `color` at `intensity` and `reach` on `row`.
fn glow(row: GuiPaintPart, color: [f32; 4], intensity: f32, reach: f32) -> GuiPaintPart {
    GuiPaintPart {
        glow_color: Some(color),
        glow_intensity: Some(intensity),
        glow_radius: Some(reach),
        glow_inner_radius: Some(reach),
        glow_falloff: Some(GLOW_FALLOFF),
        ..row
    }
}

/// The idle frame: the idle line of `variant` round the surface, cut `cut`.
fn frame(
    part: GuiPrimitivePart,
    palette: &Palette,
    variant: &Variant,
    cut: [f32; 4],
) -> GuiPaintPart {
    GuiPaintPart {
        color: Some(palette.surface),
        border_width: Some(LINE),
        border_color: Some(variant.idle),
        corner_cut: Some(cut),
        ..base(part)
    }
}

/// The hover edge on `row`: the lit line with the half glow of `reach`; also
/// under a press.
fn hover_edge(row: GuiPaintPart, variant: &Variant, reach: f32) -> GuiPaintPart {
    glow(
        GuiPaintPart {
            border_width: Some(LIT_LINE),
            border_color: Some(variant.lit),
            ..row
        },
        variant.lit,
        HOVER_GLOW,
        reach,
    )
}

/// Hover on a part: the hover edge.
fn hover(part: GuiPrimitivePart, variant: &Variant, reach: f32) -> GuiPaintPart {
    hover_edge(state(part, GuiSkinState::Hovered), variant, reach)
}

/// Press on a part: the lit fill under the hover edge.
fn press(part: GuiPrimitivePart, variant: &Variant, reach: f32) -> GuiPaintPart {
    hover_edge(
        GuiPaintPart {
            color: Some(variant.lit),
            ..state(part, GuiSkinState::Pressed)
        },
        variant,
        reach,
    )
}

/// A disabled outline: the lit line drawn in `neutral`.
fn disable(part: GuiPrimitivePart, palette: &Palette) -> GuiPaintPart {
    GuiPaintPart {
        border_color: Some(palette.neutral),
        ..state(part, GuiSkinState::Disabled)
    }
}

/// Labels: `color` at rest and `neutral` while disabled.
fn label(palette: &Palette, color: [f32; 4]) -> [GuiPaintPart; 2] {
    [
        GuiPaintPart {
            color: Some(color),
            ..base(GuiPrimitivePart::Label)
        },
        GuiPaintPart {
            color: Some(palette.neutral),
            ..state(GuiPrimitivePart::Label, GuiSkinState::Disabled)
        },
    ]
}

/// The focus ring: the lit line with the full glow of `reach` on the focused
/// part's own contour, so focus lights the control's border instead of adding
/// a second outline or changing a fill. A ring is an outline whose colour
/// styles its line unless a border colour does, so the ring sets its colour
/// and leaves the border colour to themes.
fn focus_ring(cut: [f32; 4], variant: &Variant, reach: f32) -> GuiPaintPart {
    glow(
        GuiPaintPart {
            color: Some(variant.lit),
            border_width: Some(LIT_LINE),
            corner_cut: Some(cut),
            ..base(GuiPrimitivePart::FocusRing)
        },
        variant.lit,
        FOCUS_GLOW,
        reach,
    )
}

/// A button cut `cut` with the resting label colour `label_color`: primary
/// buttons take the frame cut and their lit label, secondary buttons the
/// part cut and a text label, and a docked button no cut. A pressed label
/// takes the surface colour on the lit fill. A selected button, the checked
/// variant, fills with the lit colour in every enabled state and `neutral`
/// while disabled, with its label in the surface colour, so it keeps that
/// look under hover, press and the focus ring.
fn button(
    palette: &Palette,
    variant: &Variant,
    cut: [f32; 4],
    label_color: [f32; 4],
) -> Vec<GuiPaintPart> {
    use GuiPartVariant::Checked;
    use GuiPrimitivePart::{Background, Label};
    use GuiSkinState::Disabled;

    let mut parts = vec![
        frame(Background, palette, variant, cut),
        hover(Background, variant, FRAME_GLOW),
        press(Background, variant, FRAME_GLOW),
        disable(Background, palette),
        GuiPaintPart {
            color: Some(palette.surface),
            ..state(Label, GuiSkinState::Pressed)
        },
        focus_ring(cut, variant, FRAME_GLOW),
    ];
    parts.extend(label(palette, label_color));
    parts.extend(STATES.map(|each| {
        let color = if each == Disabled {
            palette.neutral
        } else {
            variant.lit
        };
        GuiPaintPart {
            color: Some(color),
            border_color: Some(color),
            ..qualified(Background, each, Checked)
        }
    }));
    parts.extend(STATES.map(|each| GuiPaintPart {
        color: Some(palette.surface),
        ..qualified(Label, each, Checked)
    }));
    parts
}

/// The check mark: two strokes at 45 degrees meeting below the centre of the
/// centred half-size icon rectangle, each running half its thickness past the
/// joint so the outer corner closes.
const CHECK_MARK: [[f32; 4]; 2] = [
    [0.0324, 0.5265, 0.4067, 0.9008],
    [0.2403, 0.9008, 0.9682, 0.1729],
];

/// Thickness of the check mark's strokes.
const CHECK_STROKE: f32 = 4.0;

/// A small control: the part-cut box takes the frame states, fills with the
/// lit colour while checked in every enabled state, and carries the check mark
/// in the surface colour. Its label is content text.
fn checkbox(palette: &Palette, variant: &Variant) -> Vec<GuiPaintPart> {
    use GuiPartVariant::Checked;
    use GuiPrimitivePart::{Background, Icon};
    use GuiSkinState::Disabled;

    let small = cut(PART_CUT);
    let mut parts = vec![
        frame(Background, palette, variant, small),
        hover(Background, variant, FRAME_GLOW),
        press(Background, variant, FRAME_GLOW),
    ];
    parts.extend(STATES.map(|each| {
        let color = if each == Disabled {
            palette.neutral
        } else {
            variant.lit
        };
        GuiPaintPart {
            color: Some(color),
            border_color: Some(color),
            ..qualified(Background, each, Checked)
        }
    }));
    parts.extend([
        GuiPaintPart {
            color: Some(palette.surface),
            shape: Some(1.0),
            border_width: Some(CHECK_STROKE),
            stroke_a: Some(CHECK_MARK[0]),
            stroke_b: Some(CHECK_MARK[1]),
            ..base(Icon)
        },
        focus_ring(small, variant, FRAME_GLOW),
    ]);
    parts.extend(label(palette, palette.text));
    parts
}

/// Scale of the switch block: 0.75 of the rail's height, as the icon rectangle
/// is half of it.
const SWITCH_BLOCK_SCALE: f32 = 1.5;

/// A checkbox drawn as a switch: the Background is a frame-cut rail and the
/// Icon a part-cut block at the rail's right end while on and its left end
/// while off, lit while on and `neutral` while off or disabled, and lit while
/// pressed as every pressed part. The rail takes only the hover edge under a
/// press, so the block stays visible, and keeps its surface while on instead
/// of the checkbox's lit fill. It switches off the check mark's stroke.
fn switch(palette: &Palette, variant: &Variant) -> Vec<GuiPaintPart> {
    use GuiPartVariant::{Checked, Unchecked};
    use GuiPrimitivePart::{Background, Icon};
    use GuiSkinState::{Disabled, Idle, Pressed};

    let rail = cut(CUT);
    let mut parts = vec![
        frame(Background, palette, variant, rail),
        hover(Background, variant, FRAME_GLOW),
        hover_edge(state(Background, Pressed), variant, FRAME_GLOW),
        GuiPaintPart {
            color: Some(variant.lit),
            scale: Some([SWITCH_BLOCK_SCALE; 2]),
            align_x: Some(1.0),
            corner_cut: Some(cut(PART_CUT)),
            shape: Some(0.0),
            border_width: Some(0.0),
            ..base(Icon)
        },
        GuiPaintPart {
            color: Some(palette.neutral),
            ..qualified(Icon, Disabled, Checked)
        },
        focus_ring(rail, variant, FRAME_GLOW),
    ];
    parts.extend(STATES.map(|each| GuiPaintPart {
        color: Some(palette.surface),
        border_color: Some(match each {
            Idle => variant.idle,
            Disabled => palette.neutral,
            _ => variant.lit,
        }),
        ..qualified(Background, each, Checked)
    }));
    parts.extend(STATES.map(|each| GuiPaintPart {
        color: Some(if each == Pressed {
            variant.lit
        } else {
            palette.neutral
        }),
        align_x: Some(-1.0),
        ..qualified(Icon, each, Unchecked)
    }));
    parts
}

/// A rail in the rail fill and quiet line, a solid lit value and a part-cut
/// thumb that is an outline at rest and fills while dragged; hover and focus
/// light the thumb with the moving part's glow. Disabled draws the value and
/// the thumb's outline in `neutral`.
fn slider(palette: &Palette, variant: &Variant) -> Vec<GuiPaintPart> {
    use GuiPrimitivePart::{Background, Fill, Icon};
    use GuiSkinState::Disabled;

    let thumb = cut(PART_CUT);
    vec![
        GuiPaintPart {
            color: Some(palette.rail_fill),
            border_width: Some(LINE),
            border_color: Some(palette.line),
            ..base(Background)
        },
        GuiPaintPart {
            color: Some(variant.lit),
            ..base(Fill)
        },
        GuiPaintPart {
            color: Some(palette.neutral),
            ..state(Fill, Disabled)
        },
        GuiPaintPart {
            color: Some(palette.surface),
            border_width: Some(LINE),
            border_color: Some(variant.lit),
            corner_cut: Some(thumb),
            ..base(Icon)
        },
        hover(Icon, variant, PART_GLOW),
        press(Icon, variant, PART_GLOW),
        disable(Icon, palette),
        focus_ring(thumb, variant, PART_GLOW),
    ]
}

/// Opacity of a disabled colour control's field and rails, which keep their
/// colours: dimmed rather than drawn in `neutral`, since they are the data.
const DISABLED_DATA: f32 = 0.4;

/// Peak and reach of the dark halo outside a colour control's marker and
/// thumbs, which keeps their light outline legible over any colour.
const HALO: f32 = 0.6;
const HALO_REACH: f32 = 2.0;

/// Radius of the colour control's marker, half its diameter at the looks'
/// `em`.
const MARKER_RADIUS: f32 = COLOR_MARKER_EMS * GUI_LOOK_EM / 2.0;

/// A colour control: its surfaces (Track) are data, square-cornered because a
/// cut would remove the extreme colours at the field's corners, in the quiet
/// line with the checker that the alpha rail's translucent colours show; paint
/// leaves it off the opaque field and hue rail. Hover and a press light the surface's edge with the half glow, and
/// focus its edge with the full glow, reaching outward only so no glow tints
/// the colours; a drag shows on the surface it holds. The marker is a ring, a
/// circle because it marks a point equally in every direction, and each rail's
/// thumb a part-cut bar; both are outlines in `text` that let the colour show
/// through, with a dark halo that keeps them legible over any colour. They are
/// no lit parts: in the accent they would vanish over the colours nearest it.
/// The swatch shows the colour in the quiet line over the checker. Disabled
/// dims the surfaces, draws the marker and thumbs in `neutral` and keeps the
/// swatch's colour.
fn color(palette: &Palette, variant: &Variant) -> Vec<GuiPaintPart> {
    use GuiPrimitivePart::{Background, Fill, Icon, Marker, Track};
    use GuiSkinState::{Disabled, Hovered, Pressed};

    let outward = |row: GuiPaintPart| GuiPaintPart {
        glow_inner_radius: Some(0.0),
        ..row
    };
    let mark = |part, corner_radius, corner_cut| {
        let row = GuiPaintPart {
            color: Some([0.0; 4]),
            border_width: Some(LIT_LINE),
            border_color: Some(palette.text),
            corner_radius,
            corner_cut,
            ..base(part)
        };
        GuiPaintPart {
            glow_falloff: Some(1.0),
            glow_inner_radius: Some(0.0),
            ..glow(row, palette.surface, HALO, HALO_REACH)
        }
    };
    let quiet = |part| GuiPaintPart {
        border_width: Some(LINE),
        border_color: Some(palette.line),
        checker_size: Some(CHECKER),
        ..base(part)
    };
    let mut parts = vec![
        GuiPaintPart {
            color: Some([0.0; 4]),
            ..base(Background)
        },
        quiet(Track),
        outward(hover_edge(state(Track, Hovered), variant, FRAME_GLOW)),
        outward(hover_edge(state(Track, Pressed), variant, FRAME_GLOW)),
        GuiPaintPart {
            opacity: Some(DISABLED_DATA),
            ..state(Track, Disabled)
        },
        quiet(Fill),
        mark(Marker, Some([MARKER_RADIUS; 2]), None),
        mark(Icon, None, Some(cut(PART_CUT))),
        outward(focus_ring(SQUARE, variant, FRAME_GLOW)),
    ];
    parts.extend([Marker, Icon].map(|part| GuiPaintPart {
        border_color: Some(palette.neutral),
        ..state(part, Disabled)
    }));
    parts
}

/// Thickness of a dial's value arc, which its track shares a centre line with.
const DIAL_VALUE: f32 = 4.0;

/// Length of a dial's ticks, outside its value ring.
const DIAL_TICK: f32 = 4.0;

/// Ticks mark each tenth of a dial's sweep, ends included: ten cells over
/// the 270-degree sweep, laid from half a cell before its start.
const DIAL_TICK_CELLS: f32 = 10.0 / DIAL_SWEEP;

/// Radius of the middle of a default dial's ticks: half its side, less the
/// tick ring's inset and half a tick.
const DIAL_TICK_MIDDLE: f32 = (DIAL_SIDE / 2.0 - DIAL_INSET_EMS) * GUI_LOOK_EM - DIAL_TICK / 2.0;

/// Each tick's share of its cell: the idle line's width at the ticks'
/// middle on a dial of the default size.
const DIAL_TICK_DUTY: f32 = LINE * DIAL_TICK_CELLS / (std::f32::consts::TAU * DIAL_TICK_MIDDLE);

/// Thickness of a dial's pointer.
const DIAL_POINTER: f32 = 2.0;

/// The pointer, drawn at twelve o'clock in the square of the value ring's
/// centre line and turned to the value's angle about its centre: from 0.4 of
/// the ring's radius out to the ring.
const DIAL_POINTER_MARK: [f32; 4] = [0.5, 0.3, 0.5, 0.0];

/// A dial: a frame-cut housing that takes the frame states, the focus ring
/// and, under a press, only the hover edge, so the dial stays visible while
/// dragged. Inside it a quiet tick ring and track arc in the quiet line, the
/// value arc and the pointer lit at rest, the pointer glowing as the moving
/// part while dragged, and both in `neutral` while disabled. Every ring is a
/// circle because the circle is the dial's function.
fn dial(palette: &Palette, variant: &Variant) -> Vec<GuiPaintPart> {
    use GuiPrimitivePart::{Background, Fill, Icon, Ticks, Track};
    use GuiSkinState::{Disabled, Pressed};

    let housing = cut(CUT);
    vec![
        frame(Background, palette, variant, housing),
        hover(Background, variant, FRAME_GLOW),
        hover_edge(state(Background, Pressed), variant, FRAME_GLOW),
        disable(Background, palette),
        focus_ring(housing, variant, FRAME_GLOW),
        GuiPaintPart {
            color: Some(palette.line),
            shape: Some(2.0),
            border_width: Some(DIAL_TICK),
            arc_dashes: Some([DIAL_TICK_CELLS, DIAL_TICK_DUTY]),
            ..base(Ticks)
        },
        GuiPaintPart {
            color: Some(palette.line),
            shape: Some(2.0),
            border_width: Some(LINE),
            ..base(Track)
        },
        GuiPaintPart {
            color: Some(variant.lit),
            shape: Some(2.0),
            border_width: Some(DIAL_VALUE),
            ..base(Fill)
        },
        GuiPaintPart {
            color: Some(palette.neutral),
            ..state(Fill, Disabled)
        },
        GuiPaintPart {
            color: Some(variant.lit),
            shape: Some(1.0),
            border_width: Some(DIAL_POINTER),
            stroke_a: Some(DIAL_POINTER_MARK),
            ..base(Icon)
        },
        glow(state(Icon, Pressed), variant.lit, HOVER_GLOW, PART_GLOW),
        GuiPaintPart {
            color: Some(palette.neutral),
            ..state(Icon, Disabled)
        },
    ]
}

/// A full-size frame with content text, a lit caret and the selection blue.
/// It has no pressed look: a press places the caret rather than activating
/// the field. A numeric field's step parts are divisions of it
/// ([`step_parts`]).
fn text_input(palette: &Palette, variant: &Variant) -> Vec<GuiPaintPart> {
    use GuiPrimitivePart::{Background, Caret, Selection};

    let field = cut(CUT);
    let mut parts = vec![
        frame(Background, palette, variant, field),
        hover(Background, variant, FRAME_GLOW),
        GuiPaintPart {
            color: Some(variant.lit),
            ..base(Caret)
        },
        GuiPaintPart {
            color: Some(palette.selection),
            ..base(Selection)
        },
        focus_ring(field, variant, FRAME_GLOW),
    ];
    parts.extend(label(palette, palette.text));
    parts.extend(step_parts(palette, variant, CUT));
    parts
}

/// A minus: one stroke across the middle of its square.
const MINUS_MARK: [f32; 4] = [0.0, 0.5, 1.0, 0.5];

/// The plus's upright stroke, crossing the minus at the middle.
const PLUS_MARK: [f32; 4] = [0.5, 0.0, 0.5, 1.0];

/// Thickness of the step marks' strokes.
const STEP_STROKE: f32 = 2.0;

/// A numeric field's decrement and increment parts, divisions of the one
/// field at its ends: each is clear at rest inside the idle line, which draws
/// the separator on its inner side and lies on the frame's own line
/// elsewhere, and keeps the frame's cut of `field` at the field's corner it
/// takes. Each takes the part states on its own: the hover edge with the
/// moving part's glow, the lit fill under a press, and the idle line in
/// `neutral` while disabled. Its mark, a minus or a plus stroke, is content
/// text, the surface colour on the lit fill and `neutral` while disabled, so
/// the direction at its bound reads muted. The field's focus ring stays on its
/// one outer border.
fn step_parts(palette: &Palette, variant: &Variant, field: f32) -> Vec<GuiPaintPart> {
    use GuiPrimitivePart::{Decrement, DecrementMark, Increment, IncrementMark};
    use GuiSkinState::{Disabled, Pressed};

    let clear = translucent(variant.lit, 0.0);
    let mut parts = Vec::new();
    for (cell, mark, corner, plus) in [
        (Decrement, DecrementMark, [field, 0.0, 0.0, 0.0], [0.0; 4]),
        (Increment, IncrementMark, [0.0, 0.0, field, 0.0], PLUS_MARK),
    ] {
        parts.extend([
            GuiPaintPart {
                color: Some(clear),
                border_width: Some(LINE),
                border_color: Some(variant.idle),
                corner_cut: Some(corner),
                ..base(cell)
            },
            hover(cell, variant, PART_GLOW),
            press(cell, variant, PART_GLOW),
            disable(cell, palette),
            GuiPaintPart {
                color: Some(palette.text),
                shape: Some(1.0),
                border_width: Some(STEP_STROKE),
                stroke_a: Some(MINUS_MARK),
                stroke_b: Some(plus),
                ..base(mark)
            },
            GuiPaintPart {
                color: Some(palette.surface),
                ..state(mark, Pressed)
            },
            GuiPaintPart {
                color: Some(palette.neutral),
                ..state(mark, Disabled)
            },
        ]);
    }
    parts
}

/// A content frame with rail tracks of pointed ends and solid part-cut thumbs,
/// which take the hover edge with the moving part's glow under the pointer and
/// while dragged; a bar that cannot scroll shows its track alone.
fn scroll(palette: &Palette, variant: &Variant) -> Vec<GuiPaintPart> {
    use GuiPrimitivePart::{Background, ScrollThumbX, ScrollThumbY, ScrollTrackX, ScrollTrackY};
    use GuiSkinState::{Disabled, Pressed};

    let mut parts = vec![frame(Background, palette, variant, cut(CUT))];
    for (track, thumb) in [(ScrollTrackX, ScrollThumbX), (ScrollTrackY, ScrollThumbY)] {
        parts.extend([
            GuiPaintPart {
                color: Some(palette.rail_fill),
                border_width: Some(LINE),
                border_color: Some(palette.line),
                corner_cut: Some(pointed_ends(BAR)),
                ..base(track)
            },
            GuiPaintPart {
                color: Some(variant.lit),
                corner_cut: Some(cut(PART_CUT)),
                ..base(thumb)
            },
            hover(thumb, variant, PART_GLOW),
            hover_edge(state(thumb, Pressed), variant, PART_GLOW),
            GuiPaintPart {
                opacity: Some(0.0),
                ..state(thumb, Disabled)
            },
        ]);
    }
    parts
}

/// Hover in: the transition into the hovered look.
const HOVER_IN: f32 = 0.08;

/// Hover out: the transition back into the idle look.
const HOVER_OUT: f32 = 0.12;

/// Release: the transition out of a press, which itself is immediate.
const RELEASE: f32 = 0.1;

/// A checked fill or a selection, entering or leaving.
const FILL: f32 = 0.1;

/// The switch block's travel between its ends, either way.
const SWITCH_TRAVEL: f32 = 0.16;

/// `GuiThemeMotion` easing indices.
const LINEAR: u32 = 0;
const EASE_OUT_CUBIC: u32 = 2;

fn timed(identity: GuiPartId, duration: f32, easing: u32, exit: Option<f32>) -> GuiMotionPart {
    GuiMotionPart {
        duration: Some(duration),
        easing: Some(easing),
        exit,
        ..GuiMotionPart::keyed(identity).expect("compiled part identity")
    }
}

/// The interaction timing of each of `parts`: hover in and out, an immediate
/// press released over its own duration, and an immediate disable both ways.
/// A part or state without rows, such as a focus ring or a check mark,
/// changes immediately.
fn interaction(parts: &[GuiPrimitivePart]) -> Vec<GuiMotionPart> {
    use GuiSkinState::{Disabled, Hovered, Pressed};

    parts
        .iter()
        .flat_map(|&part| {
            [
                timed(GuiPartId::base(part), HOVER_OUT, LINEAR, None),
                timed(GuiPartId::state(part, Hovered), HOVER_IN, LINEAR, None),
                timed(GuiPartId::state(part, Pressed), 0.0, LINEAR, Some(RELEASE)),
                timed(GuiPartId::state(part, Disabled), 0.0, LINEAR, Some(0.0)),
            ]
        })
        .collect()
}

/// The timing of a change of checked variant of each of `parts`, landing idle
/// or hovered; a change while pressed or disabled is immediate.
fn value_change(parts: &[GuiPrimitivePart], duration: f32, easing: u32) -> Vec<GuiMotionPart> {
    use GuiPartVariant::{Checked, Unchecked};
    use GuiSkinState::{Hovered, Idle};

    parts
        .iter()
        .flat_map(|&part| {
            [
                (Idle, Checked),
                (Idle, Unchecked),
                (Hovered, Checked),
                (Hovered, Unchecked),
            ]
            .map(|(state, variant)| {
                timed(
                    GuiPartId::variant(part, state, variant),
                    duration,
                    easing,
                    None,
                )
            })
        })
        .collect()
}

/// Buttons: the frame and its content follow the interaction timing, and a
/// selection fills over the fill duration.
fn button_motion() -> Vec<GuiMotionPart> {
    use GuiPrimitivePart::{Background, Icon, Label};

    let mut rows = interaction(&[Background, Label, Icon]);
    rows.extend(value_change(&[Background, Label], FILL, LINEAR));
    rows
}

/// A checkbox: the box fills over the fill duration and its check mark
/// appears and disappears immediately.
fn checkbox_motion() -> Vec<GuiMotionPart> {
    use GuiPrimitivePart::{Background, Label};

    let mut rows = interaction(&[Background, Label]);
    rows.extend(value_change(&[Background], FILL, LINEAR));
    rows
}

/// The switch block follows the interaction timing and travels with the
/// switch's own duration and easing. The rail keeps the checkbox's timing.
fn switch_motion() -> Vec<GuiMotionPart> {
    use GuiPrimitivePart::Icon;

    let mut rows = interaction(&[Icon]);
    rows.extend(value_change(&[Icon], SWITCH_TRAVEL, EASE_OUT_CUBIC));
    rows
}

/// A slider's rail, value and thumb follow the interaction timing; the value
/// and thumb positions follow the committed value immediately.
fn slider_motion() -> Vec<GuiMotionPart> {
    use GuiPrimitivePart::{Background, Fill, Icon, Label};

    interaction(&[Background, Fill, Icon, Label])
}

/// A dial's housing, value arc and pointer follow the interaction timing; the
/// arc and pointer follow the committed value immediately.
fn dial_motion() -> Vec<GuiMotionPart> {
    use GuiPrimitivePart::{Background, Fill, Icon};

    interaction(&[Background, Fill, Icon])
}

/// A text field's frame and text, and a numeric field's step parts and their
/// marks, each part on its own, follow the interaction timing.
fn text_input_motion() -> Vec<GuiMotionPart> {
    use GuiPrimitivePart::{Background, Decrement, DecrementMark, Increment, IncrementMark, Label};

    interaction(&[
        Background,
        Label,
        Decrement,
        Increment,
        DecrementMark,
        IncrementMark,
    ])
}

/// A colour control's surfaces follow the interaction timing; its marker and
/// thumbs change only when disabled, at once, and follow the committed value
/// immediately.
fn color_motion() -> Vec<GuiMotionPart> {
    interaction(&[GuiPrimitivePart::Track])
}

/// A scroll view's frame; its bars change immediately.
fn scroll_motion() -> Vec<GuiMotionPart> {
    interaction(&[GuiPrimitivePart::Background])
}

#[cfg(test)]
#[path = "looks_tests.rs"]
mod tests;
