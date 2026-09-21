//! Borrowed B-tree keys preserve source ordering without temporary owned URIs.

use super::{AssetSource, AssetTypeId};
use std::{borrow::Borrow, cmp::Ordering};

pub(super) struct AssetSourceLookup<'a> {
    pub kind: AssetTypeId,
    pub uri: [&'a str; 4],
    pub variant: u32,
}

pub(super) trait AssetSourceIdentity {
    fn identity(&self) -> AssetSourceLookup<'_>;
}

impl AssetSourceIdentity for AssetSource {
    fn identity(&self) -> AssetSourceLookup<'_> {
        AssetSourceLookup {
            kind: self.kind,
            uri: [&self.uri, "", "", ""],
            variant: self.variant,
        }
    }
}

impl AssetSourceIdentity for AssetSourceLookup<'_> {
    fn identity(&self) -> AssetSourceLookup<'_> {
        AssetSourceLookup {
            kind: self.kind,
            uri: self.uri,
            variant: self.variant,
        }
    }
}

impl<'a> Borrow<dyn AssetSourceIdentity + 'a> for AssetSource {
    fn borrow(&self) -> &(dyn AssetSourceIdentity + 'a) {
        self
    }
}

impl PartialEq for dyn AssetSourceIdentity + '_ {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for dyn AssetSourceIdentity + '_ {}

impl PartialOrd for dyn AssetSourceIdentity + '_ {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for dyn AssetSourceIdentity + '_ {
    fn cmp(&self, other: &Self) -> Ordering {
        let a = self.identity();
        let b = other.identity();
        a.kind
            .cmp(&b.kind)
            .then_with(|| compare_uri_fragments(a.uri, b.uri))
            .then_with(|| a.variant.cmp(&b.variant))
    }
}

/// Compare concatenated UTF-8 bytes without allocating or visiting each byte
/// through nested iterators. Most references are one contiguous immutable URI.
fn compare_uri_fragments(a: [&str; 4], b: [&str; 4]) -> Ordering {
    let mut left = a.into_iter().map(str::as_bytes).filter(|s| !s.is_empty());
    let mut right = b.into_iter().map(str::as_bytes).filter(|s| !s.is_empty());
    let mut a = left.next().unwrap_or_default();
    let mut b = right.next().unwrap_or_default();
    loop {
        if a.is_empty() || b.is_empty() {
            return a.len().cmp(&b.len());
        }
        let length = a.len().min(b.len());
        let order = a[..length].cmp(&b[..length]);
        if order != Ordering::Equal {
            return order;
        }

        a = &a[length..];
        b = &b[length..];
        if a.is_empty() {
            a = left.next().unwrap_or_default();
        }
        if b.is_empty() {
            b = right.next().unwrap_or_default();
        }
    }
}

impl super::AssetManagementService {
    /// Resolve a World-local source with the same logarithmic lookup as an owned key.
    pub(crate) fn find_source(
        &self,
        world: crate::WorldId,
        kind: AssetTypeId,
        source: &str,
        variant: u32,
    ) -> Option<super::AssetKey> {
        let mut digits = [0u8; 20];
        let mut start = digits.len();
        let mut owner = world.0;
        loop {
            start -= 1;
            digits[start] = b'0' + (owner % 10) as u8;
            owner /= 10;
            if owner == 0 {
                break;
            }
        }
        let owner = std::str::from_utf8(&digits[start..]).expect("decimal ASCII");
        let uri = if let Some(path) = source.strip_prefix("asset://") {
            ["producer://", owner, "/", path]
        } else {
            [source, "", "", ""]
        };
        let query = AssetSourceLookup {
            kind,
            uri,
            variant,
        };
        self.sources
            .get(&query as &dyn AssetSourceIdentity)
            .copied()
    }
}

impl AssetSourceIdentity for super::service::AssetDemandSelection {
    fn identity(&self) -> AssetSourceLookup<'_> {
        AssetSourceLookup {
            kind: self.kind,
            uri: [&self.source, "", "", ""],
            variant: self.variant,
        }
    }
}

impl<'a> Borrow<dyn AssetSourceIdentity + 'a> for super::service::AssetDemandSelection {
    fn borrow(&self) -> &(dyn AssetSourceIdentity + 'a) {
        self
    }
}

impl super::service::AssetDemandSelection {
    /// Mark a retained selection without copying source text for an existing key.
    pub(crate) fn mark_selected(
        selections: &mut std::collections::BTreeMap<Self, bool>,
        kind: AssetTypeId,
        source: &str,
        variant: u32,
    ) -> bool {
        let query = AssetSourceLookup {
            kind,
            uri: [source, "", "", ""],
            variant,
        };
        if let Some(seen) = selections.get_mut(&query as &dyn AssetSourceIdentity) {
            *seen = true;
            false
        } else {
            selections.insert(Self::new(kind, source, variant), true);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn fragment_order_matches_concatenated_bytes() {
        let values = [
            "",
            "a",
            "aa",
            "ab",
            "é",
            "producer://12/a",
            "producer://2/ab",
        ];
        for a in values {
            for b in values {
                for i in (0..=a.len()).filter(|&i| a.is_char_boundary(i)) {
                    for j in (0..=b.len()).filter(|&j| b.is_char_boundary(j)) {
                        assert_eq!(
                            compare_uri_fragments(
                                ["", &a[..i], "", &a[i..]],
                                [&b[..j], "", &b[j..], ""]
                            ),
                            a.cmp(b),
                            "{a:?} at {i}, {b:?} at {j}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn borrowed_fragments_match_owned_order_and_namespace() {
        let sources: BTreeMap<_, _> = [
            "",
            "producer://1/a",
            "producer://12/a",
            "producer://12/aa",
            "producer://12/é",
            "file:///a",
        ]
        .into_iter()
        .flat_map(|uri| {
            [1, 2].into_iter().flat_map(move |kind| {
                [0, 2].into_iter().map(move |variant| AssetSource {
                    kind: AssetTypeId(kind),
                    uri: uri.into(),
                    variant,
                })
            })
        })
        .enumerate()
        .map(|(i, key)| (key, i))
        .collect();
        for (key, expected) in &sources {
            for split in 0..=key.uri.len() {
                if !key.uri.is_char_boundary(split) {
                    continue;
                }
                let lookup = AssetSourceLookup {
                    kind: key.kind,
                    uri: [&key.uri[..split], "", &key.uri[split..], ""],
                    variant: key.variant,
                };
                assert_eq!(
                    sources.get(&lookup as &dyn AssetSourceIdentity),
                    Some(expected)
                );
            }
        }
        let lookup = AssetSourceLookup {
            kind: AssetTypeId(1),
            uri: ["producer://", "12", "/", "a"],
            variant: 0,
        };
        assert_eq!(
            sources.get(&lookup as &dyn AssetSourceIdentity),
            sources.get(&AssetSource {
                kind: AssetTypeId(1),
                uri: "producer://12/a".into(),
                variant: 0
            })
        );
    }
}
