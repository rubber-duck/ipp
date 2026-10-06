use super::service::AssetResourceKind;
use super::{AssetSource, AssetTypeId};
use crate::ErrorReason;
use std::collections::BTreeSet;
use std::sync::Arc;

pub(crate) fn validate_reference(
    expected_kind: Option<AssetTypeId>,
    source: &str,
) -> Result<(), ErrorReason> {
    u32::try_from(source.len()).map_err(|_| ErrorReason::Capacity)?;
    let (kind, asset, canonical) = if let Some(path) = source.strip_prefix("producer://") {
        let mut parts = path.split('/');
        let world: u64 = parts
            .next()
            .ok_or(ErrorReason::InvalidAsset)?
            .parse()
            .map_err(|_| ErrorReason::InvalidAsset)?;
        let kind: u16 = parts
            .next()
            .ok_or(ErrorReason::InvalidAsset)?
            .parse()
            .map_err(|_| ErrorReason::InvalidAsset)?;
        let asset: u64 = parts
            .next()
            .ok_or(ErrorReason::InvalidAsset)?
            .parse()
            .map_err(|_| ErrorReason::InvalidAsset)?;
        if world == 0 || parts.next().is_some() {
            return Err(ErrorReason::InvalidAsset);
        }
        (kind, asset, format!("producer://{world}/{kind}/{asset}"))
    } else if let Some(path) = source.strip_prefix("asset://") {
        let (kind, asset) = path.split_once('/').ok_or(ErrorReason::InvalidAsset)?;
        let kind: u16 = kind.parse().map_err(|_| ErrorReason::InvalidAsset)?;
        let asset: u64 = asset.parse().map_err(|_| ErrorReason::InvalidAsset)?;
        (kind, asset, format!("asset://{kind}/{asset}"))
    } else {
        return Ok(());
    };

    if expected_kind.is_some_and(|expected| expected.0 != kind)
        || asset == 0
        || asset >= 1 << 63
        || source != canonical
    {
        return Err(ErrorReason::InvalidAsset);
    }
    Ok(())
}

impl AssetSource {
    pub(crate) fn validate(&self) -> Result<(), ErrorReason> {
        validate_reference(Some(self.kind), &self.uri)
    }
}

pub(crate) fn validate_source(source: &str) -> Result<(), ErrorReason> {
    validate_reference(None, source)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct AssetDemandSelection {
    pub(crate) kind: AssetResourceKind,
    pub(crate) source: Arc<str>,
    pub(crate) variant: u32,
}

impl AssetDemandSelection {
    pub(crate) fn new(kind: AssetResourceKind, source: &Arc<str>, variant: u32) -> Self {
        Self {
            kind,
            source: source.clone(),
            variant,
        }
    }

    pub(crate) fn insert_into(
        demand: &mut BTreeSet<Self>,
        kind: AssetTypeId,
        source: &Arc<str>,
        variant: u32,
    ) {
        let query = super::source_lookup::AssetSourceLookup {
            kind,
            uri: [source, "", "", ""],
            variant,
        };
        if demand.contains(&query as &dyn super::source_lookup::AssetSourceIdentity) {
            return;
        }
        demand.insert(Self::new(kind, source, variant));
    }

    pub(crate) fn descriptor(&self) -> AssetSource {
        AssetSource {
            kind: self.kind,
            uri: self.source.clone(),
            variant: self.variant,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_sources_use_the_catalog_canonical_grammar_without_readiness_checks() {
        for source in [
            "",
            "archive!/mesh?name=ordinary text",
            "https://assets.test/pending",
            "asset://2/42",
            "producer://9/2/42",
        ] {
            assert_eq!(
                validate_reference(Some(AssetTypeId(2)), source),
                Ok(()),
                "{source}"
            );
        }
        for source in [
            "asset://ordinary-motion-A",
            "asset://materials/x",
            "asset://2/0",
            "asset://2/9223372036854775808",
            "asset://2/18446744073709551616",
            "asset://02/42",
            "asset://2/+42",
            "asset://2/042",
            "asset://2/42/extra",
            "asset://2/42?variant=1",
            "asset://1/42",
            "producer://0/2/42",
            "producer://09/2/42",
            "producer://9/1/42",
            "producer://9/2/42/extra",
        ] {
            assert_eq!(
                validate_reference(Some(AssetTypeId(2)), source),
                Err(ErrorReason::InvalidAsset),
                "{source}"
            );
        }
    }
}
