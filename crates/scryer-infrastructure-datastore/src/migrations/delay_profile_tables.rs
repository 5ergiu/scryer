//! Migration 0293: move the delay profile catalog out of settings.
//!
//! Until now every delay profile lived in one JSON array under the
//! `acquisition.delay_profiles` setting. The schema half of 0293 creates the
//! `delay_profiles` table and its facet, tag and library set tables; this hook
//! copies the stored array into them and then removes the setting, so nothing
//! is left that could drift from the tables.
//!
//! Each profile goes through the same normalizer the save path uses, so the
//! rows read back exactly as the running app would have seen them. No profile
//! is scoped to a library yet, because the JSON had no such field.
//!
//! A catalog that does not parse, or that fails validation, fails the upgrade
//! instead of being dropped: the save path never wrote such a catalog, so one
//! means the database was edited by hand, and discarding an operator's delay
//! policy without a word would grab releases they meant to hold.

use scryer_application::{
    AppError, AppResult, DELAY_PROFILE_CATALOG_KEY, DelayProfile, normalize_delay_profile,
    parse_delay_profile_catalog, validate_delay_profile_catalog,
};
use sqlx::Row;

const SETTING_VALUE_SQL_SQLITE: &str = "SELECT values_table.value_json
       FROM settings_values values_table
       JOIN settings_definitions definitions
         ON definitions.id = values_table.setting_definition_id
      WHERE definitions.scope = 'system'
        AND definitions.key_name = ?1
        AND values_table.scope = 'system'
        AND values_table.scope_id IS NULL";

const SETTING_VALUE_SQL_POSTGRES: &str = "SELECT values_table.value_json::text AS value_json
       FROM settings_values values_table
       JOIN settings_definitions definitions
         ON definitions.id = values_table.setting_definition_id
      WHERE definitions.scope = 'system'
        AND definitions.key_name = $1
        AND values_table.scope = 'system'
        AND values_table.scope_id IS NULL";

/// Parse, normalize and validate the stored catalog. `None` (no stored value)
/// imports nothing.
fn plan_import(raw: Option<&str>) -> AppResult<Vec<DelayProfile>> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(Vec::new());
    };
    if raw == "null" {
        return Ok(Vec::new());
    }
    let profiles = parse_delay_profile_catalog(raw)
        .map_err(|error| {
            AppError::Repository(format!(
                "the stored delay profile catalog is not valid JSON ({error}); \
                 fix or clear the '{DELAY_PROFILE_CATALOG_KEY}' setting and start again"
            ))
        })?
        .into_iter()
        .map(normalize_delay_profile)
        .collect::<Vec<_>>();
    validate_delay_profile_catalog(&profiles).map_err(|error| {
        AppError::Repository(format!(
            "the stored delay profile catalog is not valid ({error}); \
             fix or clear the '{DELAY_PROFILE_CATALOG_KEY}' setting and start again"
        ))
    })?;
    Ok(profiles)
}

/// `(table, column, values)` for each set a profile owns.
fn profile_sets(profile: &DelayProfile) -> [(&'static str, &'static str, &[String]); 3] {
    [
        ("delay_profile_facets", "facet", &profile.applies_to_facets),
        ("delay_profile_tags", "label", &profile.tags),
        (
            "delay_profile_libraries",
            "library_id",
            &profile.library_ids,
        ),
    ]
}

pub async fn import_delay_profile_catalog_sqlite(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> AppResult<()> {
    let raw: Option<String> = sqlx::query(SETTING_VALUE_SQL_SQLITE)
        .bind(DELAY_PROFILE_CATALOG_KEY)
        .fetch_optional(&mut **tx)
        .await
        .map_err(repo_err)?
        .map(|row| row.try_get("value_json"))
        .transpose()
        .map_err(repo_err)?;
    let profiles = plan_import(raw.as_deref())?;
    let now = chrono::Utc::now().to_rfc3339();
    for (position, profile) in profiles.iter().enumerate() {
        sqlx::query(
            "INSERT INTO delay_profiles (
                 id, name, usenet_delay_minutes, torrent_delay_minutes, preferred_protocol,
                 min_age_minutes, bypass_score_threshold, enable_usenet, enable_torrent,
                 bypass_if_highest_quality, priority, enabled, created_at, updated_at, position
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13, ?14)",
        )
        .bind(&profile.id)
        .bind(&profile.name)
        .bind(profile.usenet_delay_minutes)
        .bind(profile.torrent_delay_minutes)
        .bind(profile.preferred_protocol.as_str())
        .bind(profile.min_age_minutes)
        .bind(profile.bypass_score_threshold)
        .bind(profile.enable_usenet)
        .bind(profile.enable_torrent)
        .bind(profile.bypass_if_highest_quality)
        .bind(profile.priority)
        .bind(profile.enabled)
        .bind(&now)
        .bind(position as i64)
        .execute(&mut **tx)
        .await
        .map_err(repo_err)?;
        for (table, column, values) in profile_sets(profile) {
            for (position, value) in values.iter().enumerate() {
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "INSERT INTO {table} (profile_id, {column}, position) VALUES (?1, ?2, ?3)"
                )))
                .bind(&profile.id)
                .bind(value)
                .bind(position as i64)
                .execute(&mut **tx)
                .await
                .map_err(repo_err)?;
            }
        }
    }
    // Values first: not every shipped schema cascades them from the definition.
    sqlx::query(
        "DELETE FROM settings_values
          WHERE setting_definition_id IN (
              SELECT id FROM settings_definitions
               WHERE scope = 'system' AND key_name = ?1
          )",
    )
    .bind(DELAY_PROFILE_CATALOG_KEY)
    .execute(&mut **tx)
    .await
    .map_err(repo_err)?;
    sqlx::query("DELETE FROM settings_definitions WHERE scope = 'system' AND key_name = ?1")
        .bind(DELAY_PROFILE_CATALOG_KEY)
        .execute(&mut **tx)
        .await
        .map_err(repo_err)?;
    Ok(())
}

pub async fn import_delay_profile_catalog_postgres(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
) -> AppResult<()> {
    let raw: Option<String> = sqlx::query(SETTING_VALUE_SQL_POSTGRES)
        .bind(DELAY_PROFILE_CATALOG_KEY)
        .fetch_optional(&mut **tx)
        .await
        .map_err(repo_err)?
        .map(|row| row.try_get("value_json"))
        .transpose()
        .map_err(repo_err)?;
    let profiles = plan_import(raw.as_deref())?;
    let now = chrono::Utc::now();
    for (position, profile) in profiles.iter().enumerate() {
        sqlx::query(
            "INSERT INTO delay_profiles (
                 id, name, usenet_delay_minutes, torrent_delay_minutes, preferred_protocol,
                 min_age_minutes, bypass_score_threshold, enable_usenet, enable_torrent,
                 bypass_if_highest_quality, priority, enabled, created_at, updated_at, position
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $13, $14)",
        )
        .bind(&profile.id)
        .bind(&profile.name)
        .bind(profile.usenet_delay_minutes)
        .bind(profile.torrent_delay_minutes)
        .bind(profile.preferred_protocol.as_str())
        .bind(profile.min_age_minutes)
        .bind(profile.bypass_score_threshold)
        .bind(profile.enable_usenet)
        .bind(profile.enable_torrent)
        .bind(profile.bypass_if_highest_quality)
        .bind(profile.priority)
        .bind(profile.enabled)
        .bind(now)
        .bind(position as i64)
        .execute(&mut **tx)
        .await
        .map_err(repo_err)?;
        for (table, column, values) in profile_sets(profile) {
            for (position, value) in values.iter().enumerate() {
                sqlx::query(sqlx::AssertSqlSafe(format!(
                    "INSERT INTO {table} (profile_id, {column}, position) VALUES ($1, $2, $3)"
                )))
                .bind(&profile.id)
                .bind(value)
                .bind(position as i32)
                .execute(&mut **tx)
                .await
                .map_err(repo_err)?;
            }
        }
    }
    sqlx::query(
        "DELETE FROM settings_values
          WHERE setting_definition_id IN (
              SELECT id FROM settings_definitions
               WHERE scope = 'system' AND key_name = $1
          )",
    )
    .bind(DELAY_PROFILE_CATALOG_KEY)
    .execute(&mut **tx)
    .await
    .map_err(repo_err)?;
    sqlx::query("DELETE FROM settings_definitions WHERE scope = 'system' AND key_name = $1")
        .bind(DELAY_PROFILE_CATALOG_KEY)
        .execute(&mut **tx)
        .await
        .map_err(repo_err)?;
    Ok(())
}

fn repo_err(error: impl std::fmt::Display) -> AppError {
    AppError::Repository(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_stored_catalog_imports_nothing() {
        assert!(plan_import(None).expect("absent").is_empty());
        assert!(plan_import(Some("")).expect("blank").is_empty());
        assert!(plan_import(Some("null")).expect("null").is_empty());
        assert!(plan_import(Some("[]")).expect("empty").is_empty());
    }

    #[test]
    fn stored_profiles_are_normalized_the_way_the_save_path_does() {
        let profiles = plan_import(Some(
            r#"[{"id":" padded ","name":" Padded ","applies_to_facets":["movie","bogus","movie"],
                 "tags":["keep"," keep ","KEEP","other"]}]"#,
        ))
        .expect("catalog should import");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0].id, "padded");
        assert_eq!(profiles[0].name, "Padded");
        assert_eq!(profiles[0].applies_to_facets, vec!["movie"]);
        assert_eq!(profiles[0].tags, vec!["keep", "other"]);
        assert!(profiles[0].library_ids.is_empty());
        assert!(profiles[0].enabled, "missing toggles keep their defaults");
    }

    #[test]
    fn a_malformed_catalog_fails_the_upgrade_instead_of_being_dropped() {
        assert!(plan_import(Some("not json")).is_err());
        assert!(plan_import(Some(r#"{"id":"not-an-array"}"#)).is_err());
        assert!(
            plan_import(Some(
                r#"[{"id":"twin","name":"One"},{"id":"TWIN","name":"Two"}]"#
            ))
            .is_err(),
            "duplicate ids fail validation"
        );
    }
}
