//! Ledger of shipped migrations that must never execute again.
//!
//! Migration history is immutable: a shipped version keeps its manifest entry,
//! source, and checksum forever, because every database that already ran it
//! validates that checksum on boot. When a shipped migration turns out to be
//! destructive or unable to upgrade valid existing data, the fix is an executor-side
//! exception: the version is *recorded as applied* — same version, same
//! description, same checksum, so history stays byte-for-byte consistent — but
//! none of its steps run. A later, non-destructive migration then performs the
//! safe part of the work on every database, whether it never ran the bad
//! version or already did.
//!
//! Every entry here is a documented, one-time exception. Add to it only when a
//! migration is known to destroy or corrupt data, or block a valid upgrade.

/// A shipped migration whose steps must not execute on any database that has
/// not applied it yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KnownBadMigration {
    /// The manifest version that must be recorded without executing.
    pub version: i64,
    /// The forward migration that performs the safe replacement work.
    pub replacement_version: i64,
    /// Why the version is quarantined; logged when it is bypassed.
    pub reason: &'static str,
}

/// Every quarantined migration, oldest first.
pub const KNOWN_BAD_MIGRATIONS: &[KnownBadMigration] = &[
    KnownBadMigration {
        version: 157,
        replacement_version: 160,
        reason: "0157 'single title folder ownership' deleted media_files rows for every \
             file outside the folder it elected (majority folder, random tie-break) \
             and did so in one long transaction; a database that has not run it \
             must never run it. 0160 performs the non-destructive folder assignment \
             instead.",
    },
    KnownBadMigration {
        version: 274,
        replacement_version: 288,
        reason: "0274 deletes retired IMDb follows and their configuration. 0288 disables \
                 the unsupported follows while preserving their configuration and history.",
    },
    KnownBadMigration {
        version: 278,
        replacement_version: 287,
        reason: "0278 creates a unique literal/language index over stale search projection \
             rows that may collide under the new identity. 0287 clears the derived \
             projection before replacing the index and requests startup reprojection.",
    },
];

/// The ledger entry for `version`, if it is quarantined.
pub fn known_bad_migration(version: i64) -> Option<&'static KnownBadMigration> {
    KNOWN_BAD_MIGRATIONS
        .iter()
        .find(|entry| entry.version == version)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_versions_are_unique_and_replacements_are_newer() {
        let mut seen = std::collections::HashSet::new();
        for entry in KNOWN_BAD_MIGRATIONS {
            assert!(
                seen.insert(entry.version),
                "duplicate ledger entry {}",
                entry.version
            );
            assert!(
                entry.replacement_version > entry.version,
                "replacement for {} must run after it",
                entry.version
            );
            assert!(!entry.reason.trim().is_empty());
        }
    }

    #[test]
    fn migration_157_is_quarantined() {
        let entry = known_bad_migration(157).expect("157 is in the ledger");
        assert_eq!(entry.replacement_version, 160);
        assert!(known_bad_migration(158).is_none());
    }
}
