use crate::ParsedReleaseMetadata;

/// Build a dedup key from parsed release metadata for cross-indexer deduplication.
///
/// Two results with the same key are considered the same release from different
/// indexers. Returns an empty string if there's not enough metadata to build a
/// reliable key (in which case the result should be kept).
pub fn build_release_dedup_key(parsed: &ParsedReleaseMetadata) -> String {
    if parsed
        .parse_hints
        .iter()
        .any(|hint| hint.starts_with("stereo:") && hint.ends_with("_conflict"))
    {
        return String::new();
    }
    let group = parsed
        .release_group
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    if group.is_empty() {
        return String::new();
    }

    let quality = parsed.quality.as_deref().unwrap_or("").to_ascii_lowercase();
    let codec = parsed
        .video_codec
        .as_ref()
        .map(ToString::to_string)
        .unwrap_or_default()
        .to_ascii_lowercase();

    let episode_key = if let Some(ref ep) = parsed.episode {
        if let Some(air_date) = ep.air_date {
            format!("air{}", air_date.format("%Y-%m-%d"))
        } else if ep.release_type == crate::ParsedEpisodeReleaseType::SeasonPack {
            format!("s{}pack", ep.season.unwrap_or(0))
        } else if let Some(season) = ep.season {
            let eps = ep
                .episode_numbers
                .iter()
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(",");
            format!("s{season}e{eps}")
        } else if !ep.special_absolute_episode_numbers.is_empty() {
            format!(
                "special{}",
                ep.special_absolute_episode_numbers
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else if !ep.absolute_episode_numbers.is_empty() {
            format!(
                "abs{}",
                ep.absolute_episode_numbers
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        } else if let Some(abs) = ep.absolute_episode {
            format!("abs{abs}")
        } else {
            return String::new();
        }
    } else {
        return String::new();
    };

    let proper = if parsed.is_repack {
        "repack"
    } else if parsed.is_proper_upload {
        "proper"
    } else {
        ""
    };

    let dual = if parsed.is_dual_audio { "dual" } else { "" };
    let edition = parsed.edition.as_deref().unwrap_or("").to_ascii_lowercase();

    let base = format!("{group}|{episode_key}|{quality}|{codec}|{proper}|{dual}|{edition}");
    // Preserve the legacy key when no technical presentation was asserted.
    let Some(stereo) = parsed.stereoscopy else {
        return base;
    };
    format!(
        "{base}|stereo:{}:{}:{}:{}",
        stereo.presentation.as_str(),
        stereo.layout.map_or("", |value| value.as_str()),
        stereo.sampling.map_or("", |value| value.as_str()),
        stereo.encoding.map_or("", |value| value.as_str())
    )
}

#[cfg(test)]
mod stereo_tests {
    use super::*;
    use crate::{
        ParsedEpisodeMetadata, ParsedEpisodeReleaseType, ParsedStereoscopy, StereoEncoding,
        StereoLayout, StereoPresentation, StereoSampling,
    };

    #[test]
    fn stereo_variants_have_distinct_keys_and_conflicts_are_retained() {
        let mut parsed = ParsedReleaseMetadata::default();
        parsed.release_group = Some("Group".into());
        parsed.episode = Some(ParsedEpisodeMetadata {
            season: Some(1),
            episode_numbers: vec![1],
            release_type: ParsedEpisodeReleaseType::SingleEpisode,
            ..Default::default()
        });
        let unknown = build_release_dedup_key(&parsed);
        let mut keys = std::collections::HashSet::from([unknown]);
        for (presentation, layout, sampling, encoding) in [
            (StereoPresentation::TwoD, None, None, None),
            (StereoPresentation::ThreeD, None, None, None),
            (StereoPresentation::Mixed2d3d, None, None, None),
            (
                StereoPresentation::ThreeD,
                Some(StereoLayout::SideBySide),
                Some(StereoSampling::Half),
                None,
            ),
            (
                StereoPresentation::ThreeD,
                Some(StereoLayout::SideBySide),
                Some(StereoSampling::Full),
                None,
            ),
            (
                StereoPresentation::ThreeD,
                Some(StereoLayout::TopBottom),
                Some(StereoSampling::Half),
                None,
            ),
            (
                StereoPresentation::ThreeD,
                None,
                None,
                Some(StereoEncoding::Mvc),
            ),
        ] {
            parsed.stereoscopy = Some(ParsedStereoscopy {
                presentation,
                layout,
                sampling,
                encoding,
            });
            assert!(keys.insert(build_release_dedup_key(&parsed)));
        }
        parsed.parse_hints.push("stereo:layout_conflict".into());
        assert!(build_release_dedup_key(&parsed).is_empty());
        parsed.parse_hints.clear();
        parsed.episode = None;
        assert!(build_release_dedup_key(&parsed).is_empty());
    }
}
