use crate::ErrorReason;
use crate::components::rows::{Rows, SchemaRow};
use crate::components::schema::ComponentLifecycle;
use crate::systems::gui::GuiPartId;
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;

/// Timing of the transitions into and out of one qualified part identity's
/// appearance. Properties resolve independently through the part's chain, like
/// appearance properties: a theme's rows first, then the default look's.
#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct GuiMotionPart {
    /// Qualified GuiPartId, independent of the row slot.
    pub part: u32,
    /// Nonnegative Host seconds of the transition into this appearance.
    pub duration: Option<f32>,
    /// [`GuiMotionEasing`] index: 0 linear, 1 smoothstep, 2 ease-out cubic;
    /// absent is linear.
    pub easing: Option<u32>,
    /// Nonnegative Host seconds of the transition from this state into one of
    /// lower precedence, in place of that destination's duration.
    pub exit: Option<f32>,
}

impl GuiMotionPart {
    /// A row of one part identity.
    pub fn keyed(part: GuiPartId) -> Result<Self, ErrorReason> {
        Ok(Self {
            part: part.index().ok_or(ErrorReason::InvalidValue)?,
            ..Self::default()
        })
    }
}

/// How a transition's progress follows its elapsed time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GuiMotionEasing {
    /// Progress equals elapsed time over duration.
    #[default]
    Linear,
    /// `3t² - 2t³`: slow at both ends.
    Smoothstep,
    /// `1 - (1 - t)³`: fast at the start, settling at the end.
    EaseOutCubic,
}

impl GuiMotionEasing {
    /// The easing of a row's `easing` index.
    pub const fn from_index(index: u32) -> Option<Self> {
        match index {
            0 => Some(Self::Linear),
            1 => Some(Self::Smoothstep),
            2 => Some(Self::EaseOutCubic),
            _ => None,
        }
    }

    /// Progress at normalised elapsed time `t` in `0..=1`.
    pub fn progress(self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::Smoothstep => t * t * (3.0 - 2.0 * t),
            Self::EaseOutCubic => 1.0 - (1.0 - t).powi(3),
        }
    }
}

/// Optional motion companion on a theme entity: transition timing rows beside
/// its `GuiTheme`, over the default look's own timing.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiThemeMotion {
    /// Compact qualified part rows; never copies appearance or control state.
    #[schema(rows)]
    pub parts: Rows<GuiMotionPart>,
}

/// Rows with distinct valid part identities, nonnegative finite seconds and a
/// known easing.
fn validate_motion_rows<'a>(
    rows: impl IntoIterator<Item = &'a GuiMotionPart>,
) -> Result<(), ErrorReason> {
    let seconds = |value: Option<f32>| value.is_none_or(|value| value.is_finite() && value >= 0.0);
    let mut keys = BTreeSet::new();
    for row in rows {
        if GuiPartId::from_index(row.part).is_none()
            || !keys.insert(row.part)
            || !seconds(row.duration)
            || !seconds(row.exit)
            || row
                .easing
                .is_some_and(|value| GuiMotionEasing::from_index(value).is_none())
        {
            return Err(ErrorReason::InvalidValue);
        }
    }
    Ok(())
}

impl ComponentLifecycle for GuiThemeMotion {
    fn validate(&self) -> Result<(), ErrorReason> {
        validate_motion_rows(self.parts.iter().map(|(_, row)| row))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::systems::gui::GuiPrimitivePart;

    fn motion(write: impl FnOnce(&mut GuiMotionPart)) -> Result<(), ErrorReason> {
        let mut part = GuiMotionPart {
            part: GuiPartId::base(GuiPrimitivePart::Background)
                .index()
                .unwrap(),
            duration: Some(0.5),
            easing: Some(2),
            exit: Some(0.1),
        };
        write(&mut part);
        let mut parts = Rows::new();
        parts.push(part).unwrap();
        GuiThemeMotion {
            parts,
        }
        .validate()
    }

    #[test]
    fn motion_rows_accept_their_boundaries_and_reject_every_out_of_range_property() {
        // Zero-length transitions, every easing and absent properties are valid.
        assert_eq!(motion(|_| {}), Ok(()));
        assert_eq!(motion(|part| part.duration = Some(0.0)), Ok(()));
        assert_eq!(motion(|part| part.exit = Some(0.0)), Ok(()));
        for easing in 0..3 {
            assert_eq!(motion(|part| part.easing = Some(easing)), Ok(()));
        }
        assert_eq!(
            motion(|part| {
                part.duration = None;
                part.easing = None;
                part.exit = None;
            }),
            Ok(())
        );

        let rejected: [fn(&mut GuiMotionPart); 8] = [
            |part| part.part = GuiPartId::COUNT,
            |part| part.duration = Some(-0.1),
            |part| part.duration = Some(f32::NAN),
            |part| part.duration = Some(f32::INFINITY),
            |part| part.exit = Some(-1.0),
            |part| part.exit = Some(f32::NAN),
            |part| part.easing = Some(3),
            |part| part.easing = Some(u32::MAX),
        ];
        for (case, write) in rejected.into_iter().enumerate() {
            assert_eq!(motion(write), Err(ErrorReason::InvalidValue), "case {case}");
        }
    }

    #[test]
    fn motion_rows_reject_duplicate_part_identities() {
        let row = GuiMotionPart {
            part: GuiPartId::base(GuiPrimitivePart::Icon).index().unwrap(),
            ..Default::default()
        };
        let mut parts = Rows::new();
        parts.push(row.clone()).unwrap();
        parts.push(row).unwrap();
        assert_eq!(
            GuiThemeMotion {
                parts,
            }
            .validate(),
            Err(ErrorReason::InvalidValue)
        );
    }

    #[test]
    fn easings_meet_their_independent_curves() {
        let close = |actual: f64, expected: f64| (actual - expected).abs() < 1e-12;
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            assert!(close(GuiMotionEasing::Linear.progress(t), t));
            assert!(close(
                GuiMotionEasing::Smoothstep.progress(t),
                3.0 * t * t - 2.0 * t * t * t
            ));
            let rest = 1.0 - t;
            assert!(close(
                GuiMotionEasing::EaseOutCubic.progress(t),
                1.0 - rest * rest * rest
            ));
        }

        // The switch sheet: 58% of the travel at a quarter and 88% at half of
        // its 160 ms.
        assert!((GuiMotionEasing::EaseOutCubic.progress(40.0 / 160.0) - 0.578_125).abs() < 1e-12);
        assert!((GuiMotionEasing::EaseOutCubic.progress(80.0 / 160.0) - 0.875).abs() < 1e-12);
        assert_eq!(GuiMotionEasing::EaseOutCubic.progress(2.0), 1.0);
        assert_eq!(GuiMotionEasing::from_index(3), None);
    }
}
