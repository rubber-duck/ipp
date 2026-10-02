//! Which rows time a part's transition from one interaction key to another.

use super::runtime::GuiMotionKey;
use super::{GuiMotionEasing, GuiMotionPart, GuiThemeMotion};
use crate::systems::gui::{GuiPartId, GuiPrimitivePart, GuiSkinState};

/// The row of one part identity among `rows`.
fn find<'a>(
    mut rows: impl Iterator<Item = &'a GuiMotionPart>,
    identity: GuiPartId,
) -> Option<&'a GuiMotionPart> {
    let index = identity.index()?;
    rows.find(|row| row.part == index)
}

/// A control's motion rows: its theme's, then its kind's default look's.
#[derive(Clone, Copy)]
pub(super) struct GuiMotionRows<'a> {
    pub theme: Option<&'a GuiThemeMotion>,
    pub look: &'a [GuiMotionPart],
}

impl GuiMotionRows<'_> {
    /// The first value of `property` along `chain`: theme rows over the whole
    /// chain, then look rows over it, as appearance properties resolve.
    fn resolve<T>(
        &self,
        chain: impl Iterator<Item = GuiPartId> + Clone,
        property: impl Fn(&GuiMotionPart) -> Option<T>,
    ) -> Option<T> {
        if let Some(theme) = self.theme {
            for identity in chain.clone() {
                if let Some(value) =
                    find(theme.parts.iter().map(|(_, row)| row), identity).and_then(&property)
                {
                    return Some(value);
                }
            }
        }
        chain
            .filter_map(|identity| find(self.look.iter(), identity))
            .find_map(property)
    }

    /// Duration and easing resolved along `chain`; no duration is no transition.
    fn entering(
        &self,
        chain: impl Iterator<Item = GuiPartId> + Clone,
    ) -> Option<(f32, GuiMotionEasing)> {
        let duration = self.resolve(chain.clone(), |row| row.duration)?;
        Some((duration, self.easing(chain)))
    }

    fn easing(&self, chain: impl Iterator<Item = GuiPartId> + Clone) -> GuiMotionEasing {
        self.resolve(chain, |row| row.easing)
            .and_then(GuiMotionEasing::from_index)
            .unwrap_or_default()
    }

    /// Timing of `part`'s transition from `from` to `to`:
    ///
    /// - a change of checked variant takes the destination's variant chain;
    /// - otherwise leaving a state for one of lower precedence (disabled over
    ///   pressed over hovered over idle) takes the left state's `exit` where its
    ///   state chain declares one;
    /// - otherwise the destination's state chain, which skips variant rows.
    pub fn timing(
        &self,
        part: GuiPrimitivePart,
        from: GuiMotionKey,
        to: GuiMotionKey,
    ) -> Option<(f32, GuiMotionEasing)> {
        let states = |state: GuiSkinState| {
            [GuiPartId::state(part, state), GuiPartId::base(part)].into_iter()
        };
        if from.variant != to.variant {
            return self.entering(GuiPartId::candidates(part, to.state, to.variant));
        }

        // `GuiSkinState` orders by precedence, highest first.
        if to.state > from.state
            && let Some(exit) = self.resolve(states(from.state), |row| row.exit)
        {
            return Some((exit, self.easing(states(from.state))));
        }

        self.entering(states(to.state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::rows::Rows;
    use crate::systems::gui::GuiPartVariant;

    const BACKGROUND: GuiPrimitivePart = GuiPrimitivePart::Background;

    fn key(state: GuiSkinState, variant: Option<GuiPartVariant>) -> GuiMotionKey {
        GuiMotionKey {
            state,
            variant,
            focused: false,
            parts: 1,
            part_states: [state; crate::systems::gui::local::GUI_MAX_FOCUS_PARTS],
            steps: None,
        }
    }

    fn row(identity: GuiPartId, duration: Option<f32>, exit: Option<f32>) -> GuiMotionPart {
        GuiMotionPart {
            duration,
            exit,
            ..GuiMotionPart::keyed(identity).unwrap()
        }
    }

    #[test]
    fn entering_takes_the_destination_and_leaving_downward_takes_the_exit() {
        use GuiSkinState::*;

        let look = [
            row(GuiPartId::base(BACKGROUND), Some(0.12), None),
            row(GuiPartId::state(BACKGROUND, Hovered), Some(0.08), None),
            row(GuiPartId::state(BACKGROUND, Pressed), Some(0.0), Some(0.1)),
            row(GuiPartId::state(BACKGROUND, Disabled), Some(0.0), Some(0.0)),
            row(
                GuiPartId::variant(BACKGROUND, Hovered, GuiPartVariant::Checked),
                Some(0.3),
                None,
            ),
        ];
        let rows = GuiMotionRows {
            theme: None,
            look: &look,
        };
        let seconds = |from, to| {
            rows.timing(BACKGROUND, key(from, None), key(to, None))
                .map(|(duration, _)| duration)
        };
        let close = |actual: Option<f32>, expected: f32| {
            assert!(
                (actual.unwrap() - expected).abs() < 1e-6,
                "{actual:?} {expected}"
            )
        };

        close(seconds(Idle, Hovered), 0.08);
        close(seconds(Hovered, Idle), 0.12);
        close(seconds(Hovered, Pressed), 0.0);
        close(seconds(Pressed, Hovered), 0.1);
        close(seconds(Pressed, Idle), 0.1);
        close(seconds(Pressed, Disabled), 0.0);
        close(seconds(Disabled, Idle), 0.0);

        // A variant row times only a change of variant, not a hover over it.
        let checked = Some(GuiPartVariant::Checked);
        let timed = |from, to| {
            rows.timing(BACKGROUND, from, to)
                .map(|(duration, _)| duration)
        };
        close(timed(key(Idle, checked), key(Hovered, checked)), 0.08);
        close(timed(key(Idle, None), key(Hovered, checked)), 0.3);
        close(
            timed(
                key(Pressed, Some(GuiPartVariant::Unchecked)),
                key(Hovered, checked),
            ),
            0.3,
        );
    }

    #[test]
    fn theme_rows_sit_on_the_look_property_by_property_and_no_duration_is_no_transition() {
        use GuiSkinState::*;

        let look = [
            row(GuiPartId::base(BACKGROUND), Some(0.12), None),
            GuiMotionPart {
                easing: Some(1),
                ..row(GuiPartId::state(BACKGROUND, Hovered), Some(0.08), None)
            },
        ];
        let mut parts = Rows::new();
        parts
            .push(row(GuiPartId::state(BACKGROUND, Hovered), Some(2.0), None))
            .unwrap();
        let theme = GuiThemeMotion {
            parts,
        };
        let rows = GuiMotionRows {
            theme: Some(&theme),
            look: &look,
        };
        assert_eq!(
            rows.timing(BACKGROUND, key(Idle, None), key(Hovered, None)),
            Some((2.0, GuiMotionEasing::Smoothstep))
        );
        assert_eq!(
            rows.timing(GuiPrimitivePart::Icon, key(Idle, None), key(Hovered, None)),
            None
        );
    }
}
