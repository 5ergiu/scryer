use sqlx::{Connection, PgConnection, SqliteConnection};

enum TestDatabase {
    Sqlite(SqliteConnection),
    Postgres(PgConnection),
}

impl TestDatabase {
    async fn execute(&mut self, sql: &str) -> Result<(), sqlx::Error> {
        match self {
            Self::Sqlite(connection) => sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                .execute(connection)
                .await
                .map(|_| ()),
            Self::Postgres(connection) => sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
                .execute(connection)
                .await
                .map(|_| ()),
        }
    }

    async fn rejects(&mut self, sql: &str, sqlite_code: &str, postgres_code: &str) {
        self.execute("SAVEPOINT key_probe").await.unwrap();
        let error = self.execute(sql).await.expect_err(sql);
        let expected = match self {
            Self::Sqlite(_) => sqlite_code,
            Self::Postgres(_) => postgres_code,
        };
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some(expected),
            "{sql}: {error}"
        );
        self.execute("ROLLBACK TO SAVEPOINT key_probe; RELEASE SAVEPOINT key_probe")
            .await
            .unwrap();
    }
}

async fn assert_primary_key_behavior(db: &mut TestDatabase) {
    db.execute(
        "CREATE TABLE rule_sets (id TEXT PRIMARY KEY NOT NULL);
         CREATE TABLE location_operations (id TEXT PRIMARY KEY NOT NULL);
         INSERT INTO rule_sets VALUES ('rule'), ('other-rule');
         INSERT INTO location_operations VALUES ('operation');",
    )
    .await
    .unwrap();
    let migrations = match db {
        TestDatabase::Sqlite(_) => [
            include_str!("../../scryer/src/db/migrations/0225_tracked_rule_packs.sql"),
            include_str!("../../scryer/src/db/migrations/0234_location_transfer_progress.sql"),
        ],
        TestDatabase::Postgres(_) => [
            include_str!("../../scryer/src/db/postgres/migrations/0225_tracked_rule_packs.sql"),
            include_str!(
                "../../scryer/src/db/postgres/migrations/0234_location_transfer_progress.sql"
            ),
        ],
    };
    for migration in migrations {
        db.execute(migration).await.unwrap();
    }

    // Valid controls satisfy all other constraints before probing missing keys.
    let pack = "INSERT INTO rule_pack_installations
        (pack_id, name, version, digest, auto_update, revision, last_updated)
        VALUES ('pack', 'Pack', '1', 'digest', FALSE, 1, '2026-09-12T00:00:00Z')";
    let progress = "INSERT INTO location_transfer_progress (operation_id) VALUES ('operation')";
    let member = "INSERT INTO rule_pack_members (pack_id, template_id, rule_set_id)
        VALUES ('pack', 'template', 'rule')";
    let title = "INSERT INTO location_transfer_titles
        (operation_id, title_id, sequence, hot_rank, summary_json)
        VALUES ('operation', 'title', 1, 1, '{}')";
    for sql in [pack, progress, member, title] {
        db.execute(sql).await.unwrap();
    }
    // 0234 itself inserts the valid singleton runtime row (id = 1).
    for sql in [
        pack.replace("'pack'", "NULL"),
        pack.replace("(pack_id, name", "(name")
            .replace("('pack',", "("),
        "INSERT INTO location_transfer_runtime (id, generation) VALUES (NULL, 1)".into(),
        "INSERT INTO location_transfer_runtime (generation) VALUES (1)".into(),
        "INSERT INTO location_transfer_progress (operation_id) VALUES (NULL)".into(),
        "INSERT INTO location_transfer_progress DEFAULT VALUES".into(),
        member.replace("'pack'", "NULL"),
        member.replace("'template'", "NULL"),
        title.replace("'operation'", "NULL"),
        title.replace("'title'", "NULL"),
    ] {
        db.rejects(&sql, "1299", "23502").await;
    }
    for sql in [
        pack.to_string(),
        progress.into(),
        member.replace("'rule'", "'other-rule'"),
        title.into(),
    ] {
        db.rejects(&sql, "1555", "23505").await;
    }
    db.rejects(
        "INSERT INTO location_transfer_runtime (id, generation) VALUES (1, 2)",
        "1555",
        "23505",
    )
    .await;
    db.rejects(
        "INSERT INTO location_transfer_runtime (id, generation) VALUES (2, 1)",
        "275",
        "23514",
    )
    .await;
    db.rejects(
        "INSERT INTO location_transfer_progress (operation_id) VALUES ('missing')",
        "787",
        "23503",
    )
    .await;
}

#[tokio::test]
async fn sqlite_primary_key_migrations_reject_missing_and_duplicate_keys() {
    let mut db = TestDatabase::Sqlite(SqliteConnection::connect("sqlite::memory:").await.unwrap());
    db.execute("PRAGMA foreign_keys = ON; BEGIN").await.unwrap();
    assert_primary_key_behavior(&mut db).await;
    db.execute("ROLLBACK").await.unwrap();
}

#[tokio::test]
async fn postgres_primary_key_migrations_reject_missing_and_duplicate_keys() {
    let Ok(url) = std::env::var("SCRYER_TEST_POSTGRES_URL") else {
        eprintln!("skipped: SCRYER_TEST_POSTGRES_URL must name an isolated test database");
        return;
    };
    let mut db = TestDatabase::Postgres(PgConnection::connect(&url).await.unwrap());
    // A transaction owns the entire synthetic schema, including on assertion
    // failure when dropping the connection rolls it back.
    let schema = format!("key_migration_test_{}", uuid::Uuid::new_v4().simple());
    db.execute(&format!(
        "BEGIN; CREATE SCHEMA {schema}; SET LOCAL search_path = {schema}"
    ))
    .await
    .unwrap();
    assert_primary_key_behavior(&mut db).await;
    db.execute("ROLLBACK").await.unwrap();
}
