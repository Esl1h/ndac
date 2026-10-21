//! Property tests for merge, normalization and formatting.
#![allow(clippy::unwrap_used, reason = "tests")]

use std::{collections::BTreeMap, fmt::Write as _};

use ndac_core::config::normalize_domain;
use ndac_core::config::{
    INHERIT, format_toml, merge, merge_ids,
    types::{DomainListConfig, ItemValue, LogsConfig, PrivacyConfig, ProfileFile, SettingsConfig},
};
use proptest::prelude::*;

fn id() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9-]{0,8}"
}

fn domain() -> impl Strategy<Value = String> {
    (
        "[a-zA-Z0-9]{1,8}",
        prop::collection::vec("[a-z0-9]{1,6}", 1..3),
        any::<bool>(),
    )
        .prop_map(|(first, rest, dot)| {
            let mut d = format!("{first}.{}", rest.join("."));
            if dot {
                d.push('.');
            }
            d
        })
}

/// Marker-free profile files touching a few representative sections.
fn profile_file() -> impl Strategy<Value = ProfileFile> {
    (
        prop::option::of(prop::collection::vec(id(), 0..4)),
        prop::option::of(prop::collection::btree_map(id(), any::<bool>(), 0..4)),
        prop::option::of(any::<bool>()),
        prop::option::of("[a-z]{2}"),
    )
        .prop_map(|(blocklists, items, web3, location)| ProfileFile {
            privacy: blocklists.map(|b| PrivacyConfig {
                blocklists: Some(b),
                ..PrivacyConfig::default()
            }),
            denylist: items.map(|m| DomainListConfig {
                mode: None,
                items: Some(
                    m.into_iter()
                        .map(|(k, v)| (k, ItemValue::Active(v)))
                        .collect(),
                ),
            }),
            settings: Some(SettingsConfig {
                web3,
                logs: location.map(|l| LogsConfig {
                    location: Some(l),
                    ..LogsConfig::default()
                }),
                ..SettingsConfig::default()
            }),
            ..ProfileFile::default()
        })
}

proptest! {
    #[test]
    fn domain_normalization_is_idempotent(d in domain()) {
        let once = normalize_domain(&d).unwrap();
        prop_assert_eq!(normalize_domain(&once).unwrap(), once);
    }

    #[test]
    fn merged_id_arrays_have_no_markers_or_duplicates(
        base in prop::option::of(prop::collection::vec(id(), 0..5)),
        over in prop::collection::vec(prop_oneof![id(), Just(INHERIT.to_owned())], 0..6),
    ) {
        let merged = merge_ids(base, Some(over)).unwrap();
        prop_assert!(!merged.iter().any(|i| i == INHERIT));
        let mut dedup = merged.clone();
        dedup.sort();
        dedup.dedup();
        prop_assert_eq!(dedup.len(), merged.len());
    }

    #[test]
    fn extends_merge_is_associative(a in profile_file(), b in profile_file(), c in profile_file()) {
        let left = merge(merge(a.clone(), b.clone()), c.clone());
        let right = merge(a, merge(b, c));
        prop_assert_eq!(left, right);
    }

    #[test]
    fn merging_with_empty_is_identity_for_content(a in profile_file()) {
        prop_assert_eq!(merge(ProfileFile::default(), a.clone()), a.clone());
        prop_assert_eq!(merge(a.clone(), ProfileFile::default()), a);
    }

    #[test]
    fn fmt_is_idempotent(
        ids in prop::collection::vec(id(), 0..6),
        items in prop::collection::btree_map(domain(), any::<bool>(), 0..5),
        comment in prop::option::of("[a-z ]{0,20}"),
        name in "[a-zA-Z ]{0,12}",
    ) {
        let mut text = String::new();
        if let Some(c) = &comment {
            writeln!(text, "# {c}").unwrap();
        }
        write!(
            text,
            "name = {name:?}\nid = \"abc\"\n\n[privacy]\nblocklists = {ids:?}\n\n[denylist.items]\n"
        )
        .unwrap();
        let map: BTreeMap<_, _> = items.into_iter().rev().collect();
        for (k, v) in map.iter().rev() {
            writeln!(text, "{k:?} = {v}").unwrap();
        }
        let once = format_toml(&text).unwrap();
        prop_assert_eq!(format_toml(&once).unwrap(), once.clone());
        // Formatting never changes meaning.
        let before: toml::Table = text.parse().unwrap();
        let after: toml::Table = once.parse().unwrap();
        let sorted = |t: &toml::Table| {
            let mut v = t["privacy"]["blocklists"].as_array().unwrap().clone();
            v.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
            v
        };
        prop_assert_eq!(sorted(&before), sorted(&after));
        prop_assert_eq!(&before["denylist"], &after["denylist"]);
        prop_assert_eq!(&before["name"], &after["name"]);
    }
}
