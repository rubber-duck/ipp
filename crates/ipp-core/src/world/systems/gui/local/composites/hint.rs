//! Hint timing on the Host clock.
//!
//! A hint is an overlay whose `GuiOverlay.mode` is hint, a child of the
//! control it describes. While a pointer hovers that control the hint opens
//! after [`GUI_HINT_HOVER_DELAY`], and while the control holds visible focus
//! after [`GUI_HINT_FOCUS_DELAY`]; a hovered hint parent wins over a focused
//! one. When its parent stops asking for it, the hint closes after
//! [`GUI_HINT_GRACE`], so a pointer crossing the gap to a neighbour does not
//! flicker it. A press on its parent closes it at once, and a hint that
//! closes otherwise while still asked for, as by Escape or a client write,
//! stays closed until its parent lets it go. One hint is open per canvas: a
//! hint opening closes the previous one.
//!
//! Delays count the Host frame deltas the World receives, so a paused Host
//! holds them. The System evaluates hints only while a delay or grace runs or
//! after hover, focus or an overlay changed.

use crate::EntityId;
use crate::systems::gui::local::GuiEntityTarget;
use crate::world::WorldSimulationState;

/// Seconds a pointer hovers a hint's parent before the hint opens.
pub(crate) const GUI_HINT_HOVER_DELAY: f64 = 0.4;

/// Seconds a hint's parent holds visible focus before the hint opens.
pub(crate) const GUI_HINT_FOCUS_DELAY: f64 = 0.3;

/// Seconds a hint stays open after its parent stops asking for it.
pub(crate) const GUI_HINT_GRACE: f64 = 0.1;

/// Seconds a delay may fall short and still count as run out: frame deltas
/// do not sum to the delays exactly.
const SLACK: f64 = 1e-6;

/// A hint overlay and the control it describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::world::systems::gui::local) struct GuiHint {
    pub overlay: EntityId,
    pub parent: GuiEntityTarget,
}

/// What hover and focus ask of the hints this frame.
#[derive(Clone, Copy, Debug, Default)]
pub(in crate::world::systems::gui::local) struct GuiHintRequest {
    /// The hint to open, if a hovered or visibly focused control has one.
    pub wanted: Option<GuiHint>,
    /// Seconds it waits before opening.
    pub delay: f64,
    /// Whether a pointer presses its parent.
    pub pressed: bool,
}

/// The hint the System opened, the one waiting to open and the one closed
/// while still asked for.
#[derive(Default)]
pub(in crate::world::systems::gui::local) struct GuiHintState {
    /// The open hint, with the grace left once its parent stopped asking.
    shown: Option<(GuiHint, Option<f64>)>,
    /// The hint waiting to open, with the delay left.
    waiting: Option<(GuiHint, f64)>,
    /// A hint closed while asked for; it stays closed until it is not.
    dismissed: Option<GuiHint>,
    /// The GUI presentation revision hover and focus last read.
    revision: u64,
}

impl GuiHintState {
    /// Whether hints need evaluating: a delay or grace runs, or hover or
    /// focus changed since `revision`.
    pub fn pending(&self, revision: u64) -> bool {
        self.waiting.is_some()
            || self.shown.is_some_and(|(_, grace)| grace.is_some())
            || revision != self.revision
    }

    /// Advance the delays by `dt` seconds toward `request`, adding to
    /// `writes` each hint overlay to open (true) or close (false).
    pub fn step(
        &mut self,
        world: &WorldSimulationState,
        request: GuiHintRequest,
        dt: f64,
        revision: u64,
        writes: &mut Vec<(EntityId, bool)>,
    ) {
        self.revision = revision;

        // A shown hint something else closed, such as Escape, a client or a
        // hidden ancestor, stays closed while asked for; its own field is
        // closed too, so showing the ancestor again does not open it.
        if let Some((hint, _)) = self.shown
            && !super::overlay::is_open(world, hint.overlay)
        {
            writes.push((hint.overlay, false));
            self.shown = None;
            self.dismissed = Some(hint);
        }
        let mut wanted = request.wanted;
        if self.dismissed.is_some() && self.dismissed != wanted {
            self.dismissed = None;
        }
        if request.pressed {
            self.dismissed = wanted;
        }
        if wanted.is_some() && wanted == self.dismissed {
            wanted = None;
        }

        // The shown hint stays while asked for and closes after its grace,
        // or at once when its parent was pressed.
        if let Some((hint, grace)) = self.shown {
            let left = if wanted == Some(hint) {
                None
            } else if self.dismissed == Some(hint) {
                Some(0.0)
            } else {
                Some(grace.map_or(GUI_HINT_GRACE, |left| left - dt))
            };
            if left.is_some_and(|left| left <= SLACK) {
                writes.push((hint.overlay, false));
                self.shown = None;
            } else {
                self.shown = Some((hint, left));
            }
        }

        // The wanted hint waits out its delay, then replaces the shown one.
        let shown = self.shown.map(|(hint, _)| hint);
        self.waiting = match (wanted, self.waiting) {
            (Some(hint), _) if shown == Some(hint) => None,
            (Some(hint), Some((waiting, left))) if waiting == hint => Some((hint, left - dt)),
            (Some(hint), _) => Some((hint, request.delay)),
            (None, _) => None,
        };
        if let Some((hint, left)) = self.waiting
            && left <= SLACK
        {
            if let Some(previous) = shown {
                writes.push((previous.overlay, false));
            }
            writes.push((hint.overlay, true));
            self.shown = Some((hint, None));
            self.waiting = None;
        }
    }
}
