//! What survives of the old upgrade policy: the repack-group rule.
//!
//! `evaluate_upgrade`, `UpgradeDecision` and the grab-side upgrade cooldown are
//! gone — every comparative decision now goes through [`crate::admission`],
//! which both grab and import call, and which is covered by
//! `admission_tests.rs`. Sonarr has no recency cooldown either, so nothing
//! refuses an admitted release for having been searched recently.

use super::*;

// ── D16: the repack-group rule, scoped like Sonarr's ──────────────────────

mod repack {
    use super::*;
    use crate::admission::{
        AdmissionScope, AdmissionSubject, CandidateFacts, Incumbent, tier_sort_key,
    };

    fn candidate(release_title: &str) -> IndexerSearchResult {
        IndexerSearchResult {
            indexer_id: None,
            source: "nzbgeek".to_string(),
            title: release_title.to_string(),
            link: None,
            download_url: None,
            source_kind: None,
            size_bytes: None,
            published_at: None,
            thumbs_up: None,
            thumbs_down: None,
            indexer_languages: None,
            indexer_subtitles: None,
            indexer_grabs: None,
            password_hint: None,
            parsed_release_metadata: Some(crate::parse_release_metadata(release_title)),
            quality_profile_decision: None,
            extra: Default::default(),
            response_attributes: Default::default(),
            guid: None,
            info_url: None,
            provenance: None,
            candidate_token: None,
            queue_scope: None,
            coverage_scope: None,
            auto_eligible: None,
            auto_decision_code: None,
            auto_decision_summary: None,
        }
    }

    fn occupied(tier_index: Option<usize>, revision: i32, group: Option<&str>) -> AdmissionSubject {
        AdmissionSubject::new(
            AdmissionScope::Episodes(vec!["ep-01".to_string()]),
            [(
                Incumbent {
                    tier_index,
                    revision,
                    file_id: "file-1".to_string(),
                    file_path: "/data/TV/Show/Season 01/file-1.mkv".to_string(),
                    release_group: group.map(str::to_string),
                    score: 900,
                    covers: vec!["ep-01".to_string()],
                    created_at: "2026-01-01T00:00:00Z".to_string(),
                },
                true,
            )],
        )
    }

    fn empty() -> AdmissionSubject {
        AdmissionSubject::new(AdmissionScope::Episodes(vec!["ep-01".to_string()]), [])
    }

    /// The repack the group itself published, over its own encode. That is the
    /// one case the rule lets through.
    #[test]
    fn a_repack_from_the_same_group_is_allowed() {
        let candidate = candidate("Show.S01E01.1080p.WEB-DL.REPACK-GRP");
        assert!(!repack_group_mismatch(
            &candidate,
            CandidateFacts::new(Some(1), 1, 900),
            &occupied(Some(1), 0, Some("grp")),
        ));
    }

    /// …and the defect it exists to stop: another group's fix for problems this
    /// library does not have.
    #[test]
    fn a_same_tier_repack_from_another_group_is_skipped() {
        let candidate = candidate("Show.S01E01.1080p.WEB-DL.REPACK-OTHER");
        assert!(repack_group_mismatch(
            &candidate,
            CandidateFacts::new(Some(1), 1, 900),
            &occupied(Some(1), 0, Some("grp")),
        ));
    }

    /// A REPACK filling a **missing** episode is just a release. Sonarr only
    /// enters the group comparison inside `IsRevisionUpgrade`, so an unoccupied
    /// scope never reaches it — and the old rule refused an unparsed-group
    /// repack here before it ever looked for an existing file.
    #[test]
    fn a_repack_filling_a_missing_episode_is_never_skipped() {
        for release in [
            "Show.S01E01.1080p.WEB-DL.REPACK-GRP",
            // No parseable group at all: still fine, there is nothing to fix.
            "Show.S01E01.1080p.WEB-DL.REPACK",
        ] {
            assert!(
                !repack_group_mismatch(
                    &candidate(release),
                    CandidateFacts::new(Some(1), 1, 900),
                    &empty(),
                ),
                "`{release}` had nothing to be a false upgrade over"
            );
        }
    }

    /// A repack of a **better tier** is a quality upgrade that happens to carry
    /// the token. Sonarr's rule is scoped to same-quality revision upgrades, so
    /// the group never enters into it.
    #[test]
    fn a_cross_tier_repack_is_allowed_whatever_the_groups_say() {
        let candidate = candidate("Show.S01E01.2160p.WEB-DL.REPACK-OTHER");
        assert!(!repack_group_mismatch(
            &candidate,
            CandidateFacts::new(Some(0), 1, 900),
            &occupied(Some(1), 0, Some("grp")),
        ));
    }

    /// Not a revision upgrade — the file on disk is already a repack — so the
    /// candidate is judged on score like anything else, not on its group.
    #[test]
    fn a_repack_that_is_not_a_revision_upgrade_is_left_to_the_score() {
        let candidate = candidate("Show.S01E01.1080p.WEB-DL.REPACK-OTHER");
        assert!(!repack_group_mismatch(
            &candidate,
            CandidateFacts::new(Some(1), 1, 900),
            &occupied(Some(1), 1, Some("grp")),
        ));
    }

    /// An unknown group on either side is a mismatch: the one fact that would
    /// make the repack legitimate cannot be established.
    #[test]
    fn an_unknown_group_on_either_side_is_a_mismatch() {
        // Incumbent's group unknown.
        assert!(repack_group_mismatch(
            &candidate("Show.S01E01.1080p.WEB-DL.REPACK-GRP"),
            CandidateFacts::new(Some(1), 1, 900),
            &occupied(Some(1), 0, None),
        ));
        // Candidate's group unknown.
        assert!(repack_group_mismatch(
            &candidate("Show.S01E01.1080p.WEB-DL.REPACK"),
            CandidateFacts::new(Some(1), 1, 900),
            &occupied(Some(1), 0, Some("grp")),
        ));
    }

    /// A release that is not a repack never reaches the rule, whatever else is
    /// true of it.
    #[test]
    fn a_plain_release_is_not_a_repack() {
        assert!(!repack_group_mismatch(
            &candidate("Show.S01E01.1080p.WEB-DL-OTHER"),
            CandidateFacts::new(Some(1), 0, 900),
            &occupied(Some(1), 0, Some("grp")),
        ));
    }

    /// Both sides unlisted is "same tier" for this rule, exactly as
    /// `admission::tier_cmp` reads it.
    #[test]
    fn two_unlisted_qualities_count_as_the_same_tier() {
        assert_eq!(tier_sort_key(None), usize::MAX);
        assert!(repack_group_mismatch(
            &candidate("Show.S01E01.REPACK-OTHER"),
            CandidateFacts::new(None, 1, 900),
            &occupied(None, 0, Some("grp")),
        ));
    }
}
