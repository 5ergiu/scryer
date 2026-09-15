use chrono::{DateTime, Duration, NaiveDate, Utc};

use crate::scoring_weights::ScoringPersona;
use crate::types::IndexerSearchResult;

/// Episodes become searchable six hours before air time.
pub(crate) const EPISODE_PRE_AIR_WINDOW_HOURS: i64 = 6;

/// Configurable thresholds for the acquisition upgrade policy.
///
/// `cross_tier_min_delta` is gone. It existed because the quality tier used to
/// be worth 3200/900/300 *inside* the score, so a whole-tier upgrade showed up
/// as a delta above 1000 and the churn threshold had to be relaxed for it. Tier
/// is now compared before score, in [`crate::admission`], so a delta only ever
/// describes a same-tier comparison and one number covers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcquisitionThresholds {
    pub upgrade_cooldown_hours: i64,
    pub same_tier_min_delta: i32,
    pub forced_upgrade_delta_bypass: i32,
}

impl Default for AcquisitionThresholds {
    fn default() -> Self {
        Self::for_persona(&ScoringPersona::Balanced)
    }
}

impl AcquisitionThresholds {
    /// Build thresholds tuned to the given scoring persona.
    pub fn for_persona(persona: &ScoringPersona) -> Self {
        match persona {
            ScoringPersona::Audiophile => Self {
                upgrade_cooldown_hours: 12,
                same_tier_min_delta: 50,
                forced_upgrade_delta_bypass: 200,
            },
            ScoringPersona::Balanced | ScoringPersona::Compatible => Self {
                upgrade_cooldown_hours: 24,
                same_tier_min_delta: 200,
                forced_upgrade_delta_bypass: 400,
            },
            ScoringPersona::Efficient => Self {
                upgrade_cooldown_hours: 24,
                same_tier_min_delta: 150,
                forced_upgrade_delta_bypass: 500,
            },
        }
    }
}

pub(crate) fn parse_schedule_baseline_date(baseline_date: Option<&str>) -> Option<DateTime<Utc>> {
    baseline_date.and_then(|d| {
        // Try RFC 3339 first, then fall back to "YYYY-MM-DD" (midnight UTC).
        DateTime::parse_from_rfc3339(d)
            .map(|dt| dt.with_timezone(&Utc))
            .ok()
            .or_else(|| {
                NaiveDate::parse_from_str(d, "%Y-%m-%d")
                    .ok()
                    .and_then(|nd| nd.and_hms_opt(0, 0, 0))
                    .map(|ndt| ndt.and_utc())
            })
    })
}

pub(crate) fn episode_search_window_is_open(
    baseline_date: Option<&str>,
    now: &DateTime<Utc>,
) -> bool {
    parse_schedule_baseline_date(baseline_date)
        .is_some_and(|baseline| *now >= baseline - Duration::hours(EPISODE_PRE_AIR_WINDOW_HOURS))
}

/// How old a file has to be before Scryer stops chasing PROPERs for it.
///
/// Sonarr's `ProperSpecification`: `file.DateAdded < DateTime.Today.AddDays(-7)`
/// rejects `ProperForOldFile`. The reasoning is that a PROPER posted a week
/// after the fact is usually a re-release of an old encode rather than a fix an
/// operator is waiting for, and the bandwidth is better spent elsewhere.
pub(crate) const PROPER_MAX_FILE_AGE_DAYS: i64 = 7;

/// Whether an incumbent landed before the PROPER window closed.
///
/// Measured from **UTC midnight** rather than Sonarr's local `DateTime.Today`,
/// because every timestamp Scryer stores is RFC 3339 UTC and reading a stored
/// instant against a local calendar day would make the guard fire at a
/// different age depending on the host's zone.
pub(crate) fn file_predates_proper_window(created_at: Option<&str>, now: &DateTime<Utc>) -> bool {
    let Some(created) = parse_schedule_baseline_date(created_at) else {
        // An unparseable or absent import time is not evidence of age. The
        // guard only ever *refuses*, so the safe reading is "recent enough".
        return false;
    };
    let Some(today) = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|day| day.and_utc())
    else {
        return false;
    };
    created < today - Duration::days(PROPER_MAX_FILE_AGE_DAYS)
}

/// Grace window before an air date at which a season stops counting as "still
/// airing" for pack purposes. Sonarr's `FullSeasonSpecification` uses
/// `AirDateUtc > UtcNow.AddHours(24)`: the finale's own air day does not block
/// the pack, because by the time a pack for it is posted the episode exists.
pub(crate) const SEASON_PACK_AIRED_GRACE_HOURS: i64 = 24;

/// Whether an episode is far enough in the future that no release can contain it.
///
/// An episode with **no air date** is not unaired: an unknown schedule is not
/// evidence of a future one, and treating it as such would block packs for every
/// series whose metadata is thin. Only a date more than
/// [`SEASON_PACK_AIRED_GRACE_HOURS`] ahead counts.
///
/// A **date-only** air date is compared as the *end* of that day. Sonarr reads
/// `AirDateUtc`, a real timestamp; `parse_schedule_baseline_date` turns
/// `YYYY-MM-DD` into midnight UTC, which is up to a full day earlier than the
/// broadcast. Taking midnight literally means a finale airing tomorrow at 21:00
/// looks ~12 h away, clears the 24 h grace, and the pack is admitted the day
/// before the episode exists — precisely the partial fetch this rule prevents.
pub(crate) fn episode_is_unaired(air_date: Option<&str>, now: &DateTime<Utc>) -> bool {
    let Some(air) = parse_schedule_baseline_date(air_date) else {
        return false;
    };
    let air = if air_date_is_date_only(air_date) {
        air + Duration::hours(24)
    } else {
        air
    };
    air > *now + Duration::hours(SEASON_PACK_AIRED_GRACE_HOURS)
}

/// `true` when the catalog gave us a day but not a time.
fn air_date_is_date_only(air_date: Option<&str>) -> bool {
    air_date.is_some_and(|value| {
        let value = value.trim();
        DateTime::parse_from_rfc3339(value).is_err()
            && NaiveDate::parse_from_str(value, "%Y-%m-%d").is_ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audiophile_thresholds_are_aggressive() {
        let t = AcquisitionThresholds::for_persona(&ScoringPersona::Audiophile);
        assert_eq!(t.same_tier_min_delta, 50);
        assert_eq!(t.upgrade_cooldown_hours, 12);
        assert_eq!(t.forced_upgrade_delta_bypass, 200);
    }

    #[test]
    fn test_balanced_thresholds_are_conservative() {
        let t = AcquisitionThresholds::for_persona(&ScoringPersona::Balanced);
        assert_eq!(t.same_tier_min_delta, 200);
        assert_eq!(t.upgrade_cooldown_hours, 24);
    }

    #[test]
    fn test_efficient_thresholds_moderate() {
        let t = AcquisitionThresholds::for_persona(&ScoringPersona::Efficient);
        assert_eq!(t.same_tier_min_delta, 150);
        assert_eq!(t.forced_upgrade_delta_bypass, 500);
    }

    #[test]
    fn test_compatible_matches_balanced() {
        let balanced = AcquisitionThresholds::for_persona(&ScoringPersona::Balanced);
        let compatible = AcquisitionThresholds::for_persona(&ScoringPersona::Compatible);
        assert_eq!(balanced, compatible);
    }
}

/// Whether a REPACK candidate must be skipped because it comes from a
/// different group than the file it would replace.
///
/// A REPACK is a group re-releasing its **own** encode to fix their own
/// mistake. Another group's repack fixes problems you do not have, so it is a
/// false upgrade — Sonarr's `RepackSpecification`.
///
/// Scoped exactly the way Sonarr scopes it (D16), which is the whole change
/// here. The rule only engages where a repack could actually be a false
/// upgrade: an existing primary file in the same tier that the candidate is a
/// *revision* upgrade over. Everything else falls through and is accepted —
/// a REPACK filling a **missing** episode, a REPACK of a better tier, a
/// same-revision release that merely carries the token.
///
/// The old shape asked a title-wide file list a scope-shaped question through
/// `SubmissionScope::episode_id()`, which is `Some` only for a single-episode
/// scope: every pack, batch, title and link scope compared against whichever
/// row the store happened to return first. It also returned "skip" for an
/// unparsed candidate group *before* looking for an existing file, so a repack
/// with no group could never fill a missing episode. Both go away by asking the
/// admission subject instead: it already holds exactly the primary files in the
/// way, each with its tier, revision and release group.
pub(crate) fn repack_group_mismatch(
    candidate: &IndexerSearchResult,
    candidate_facts: crate::admission::CandidateFacts,
    subject: &crate::admission::AdmissionSubject,
) -> bool {
    let Some(parsed) = candidate
        .parsed_release_metadata
        .as_ref()
        .filter(|parsed| parsed.is_repack)
    else {
        return false;
    };

    let candidate_group = parsed
        .release_group
        .as_deref()
        .map(str::trim)
        .filter(|group| !group.is_empty());

    subject.incumbents().iter().any(|incumbent| {
        // Sonarr's `IsRevisionUpgrade`: same quality, higher revision. Equality
        // of `Option<usize>` is `admission::tier_cmp(..) == Equal` — both listed
        // at the same position, or both unlisted.
        if candidate_facts.tier_index != incumbent.tier_index
            || candidate_facts.revision <= incumbent.revision
        {
            return false;
        }
        // An unknown group on either side is a mismatch, because the one thing
        // that would make this repack legitimate cannot be established.
        match (candidate_group, incumbent.release_group.as_deref()) {
            (Some(candidate_group), Some(incumbent_group)) => {
                !candidate_group.eq_ignore_ascii_case(incumbent_group)
            }
            _ => true,
        }
    })
}

#[cfg(test)]
#[path = "acquisition_policy_tests.rs"]
mod acquisition_policy_tests;

#[cfg(test)]
mod unaired_tests {
    use super::*;
    use chrono::Duration;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-21T12:00:00Z")
            .expect("fixture timestamp")
            .with_timezone(&Utc)
    }

    /// Sonarr's grace window: `AirDateUtc > UtcNow.AddHours(24)`. The finale's
    /// own air day does not block the pack, because by the time a pack for it is
    /// posted the episode exists.
    #[test]
    fn only_air_dates_beyond_the_grace_window_count_as_unaired() {
        let now = now();
        let at = |offset: Duration| (now + offset).to_rfc3339();

        assert!(episode_is_unaired(Some(&at(Duration::days(7))), &now));
        assert!(episode_is_unaired(Some(&at(Duration::hours(25))), &now));
        assert!(!episode_is_unaired(Some(&at(Duration::hours(24))), &now));
        assert!(!episode_is_unaired(Some(&at(Duration::hours(6))), &now));
        assert!(!episode_is_unaired(Some(&at(-Duration::days(30))), &now));
    }

    /// An unknown schedule is not evidence of a future one. Treating a missing
    /// air date as unaired would block packs for every series with thin
    /// metadata, which is most anime.
    #[test]
    fn an_episode_without_an_air_date_is_not_unaired() {
        assert!(!episode_is_unaired(None, &now()));
        assert!(!episode_is_unaired(Some(""), &now()));
        assert!(!episode_is_unaired(Some("not-a-date"), &now()));
    }

    /// Date-only air dates (the common catalog shape) are compared as the **end**
    /// of the air day, not midnight.
    ///
    /// `now` is 2026-08-21 12:00 UTC, so the grace window closes at
    /// 2026-08-22 12:00. An episode dated 2026-08-22 broadcasts at some unknown
    /// hour of that day; midnight would put it inside the window and admit a
    /// pack for a season whose next episode has not aired. End-of-day
    /// (2026-08-23 00:00) keeps it out, which is what the rule is for.
    #[test]
    fn date_only_air_dates_are_measured_from_the_end_of_the_air_day() {
        let now = now();
        assert!(episode_is_unaired(Some("2026-08-25"), &now));
        assert!(
            episode_is_unaired(Some("2026-08-22"), &now),
            "an episode airing at some hour of tomorrow must still block a pack"
        );
        // Today's episode: the day ends 12 h from now, inside the grace window.
        assert!(!episode_is_unaired(Some("2026-08-21"), &now));
        assert!(!episode_is_unaired(Some("2026-08-01"), &now));
    }

    /// A real timestamp is taken at face value — the end-of-day correction is
    /// only for values that carry no time.
    #[test]
    fn a_timestamped_air_date_is_not_pushed_to_the_end_of_its_day() {
        let now = now();
        // 2026-08-22 06:00 is 18 h away: inside the grace window, so not unaired.
        // A date-only "2026-08-22" would be, which is the whole distinction.
        assert!(!episode_is_unaired(Some("2026-08-22T06:00:00Z"), &now));
        assert!(episode_is_unaired(Some("2026-08-22"), &now));
    }
}
