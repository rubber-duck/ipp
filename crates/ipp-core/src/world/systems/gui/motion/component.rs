use crate::ErrorReason;
use crate::components::rows::{Rows, SchemaRow};
use crate::components::schema::ComponentLifecycle;
use crate::services::asset_management::{AssetSource, service::AssetDemandSelection};
use crate::systems::animation::ANIMATION_TYPE;
use crate::systems::gui::GuiPartId;
use ipp_schema_derive::SchemaComponent;
use std::collections::BTreeSet;

/// Sparse motion properties resolved independently through the appearance part chain.
#[derive(Clone, Debug, Default, PartialEq, SchemaRow)]
pub struct GuiMotionPart {
    /// Qualified GuiPartId, independent of the row slot.
    pub part: u32,
    /// Immutable animation clip; tracks are color, opacity, scale, optional alignment.
    pub source: Option<AssetSource>,
    /// Nonnegative fade duration in Host seconds.
    pub duration: Option<f32>,
    /// Zero is linear; one is smoothstep.
    pub easing: Option<u32>,
    /// First of the consecutive typed tracks.
    pub track: Option<u32>,
    /// Nonnegative clip time representing the resolved destination.
    pub time: Option<f32>,
}

/// Optional motion companion on the entity referenced by GuiSkin.theme.
#[repr(C)]
#[derive(Clone, Debug, Default, PartialEq, SchemaComponent)]
pub struct GuiThemeMotion {
    /// Compact qualified part rows; never copies appearance or control state.
    #[schema(rows)]
    pub parts: Rows<GuiMotionPart>,
}

impl ComponentLifecycle for GuiThemeMotion {
    fn validate(&self) -> Result<(), ErrorReason> {
        let mut keys = BTreeSet::new();
        for (_, row) in self.parts.iter() {
            if GuiPartId::from_index(row.part).is_none()
                || !keys.insert(row.part)
                || row
                    .source
                    .as_ref()
                    .is_some_and(|source| source.kind != ANIMATION_TYPE)
                || row
                    .duration
                    .is_some_and(|value| !value.is_finite() || value < 0.0)
                || row
                    .time
                    .is_some_and(|value| !value.is_finite() || value < 0.0)
                || row.easing.is_some_and(|value| value > 1)
                || row
                    .track
                    .is_some_and(|value| value.checked_add(2).is_none())
            {
                return Err(ErrorReason::InvalidValue);
            }
        }
        Ok(())
    }

    fn resource_demand(&self, demand: &mut BTreeSet<AssetDemandSelection>) {
        for (_, row) in self.parts.iter() {
            if let Some(source) = &row.source {
                AssetDemandSelection::insert_into(demand, source.kind, &source.uri, source.variant);
            }
        }
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
            source: Some(AssetSource {
                kind: ANIMATION_TYPE,
                uri: "fixture:///motion".into(),
                variant: 0,
            }),
            duration: Some(0.5),
            easing: Some(1),
            track: Some(u32::MAX - 2),
            time: Some(0.0),
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
        // The last base track that still addresses consecutive colour, opacity
        // and scale tracks, zero-length fades and both easings are valid.
        assert_eq!(motion(|_| {}), Ok(()));
        assert_eq!(motion(|part| part.duration = Some(0.0)), Ok(()));
        assert_eq!(motion(|part| part.easing = Some(0)), Ok(()));

        let rejected: [fn(&mut GuiMotionPart); 10] = [
            |part| part.part = GuiPartId::COUNT,
            |part| {
                part.source = Some(AssetSource {
                    kind: crate::TEXTURE_TYPE,
                    uri: "fixture:///texture".into(),
                    variant: 0,
                })
            },
            |part| part.duration = Some(-0.1),
            |part| part.duration = Some(f32::NAN),
            |part| part.duration = Some(f32::INFINITY),
            |part| part.time = Some(-1.0),
            |part| part.time = Some(f32::NAN),
            |part| part.easing = Some(2),
            |part| part.track = Some(u32::MAX - 1),
            |part| part.track = Some(u32::MAX),
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
}
