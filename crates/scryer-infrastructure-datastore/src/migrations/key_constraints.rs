//! Authoring guard for SQL assets, shared by the build script and migration tools.
//!
//! Every ordinary table must declare a primary key and explicit NOT NULL on each
//! component, in both dialects. This includes INTEGER keys: spelling the invariant
//! out avoids SQLite's INTEGER/BIGINT and inline/composite primary-key differences.
//! Released SQL is immutable; 0211 is the fixed 0.19.17 boundary, not a moving
//! exemption. New baselines must satisfy the same rule as new migrations.

use std::collections::BTreeMap;

pub(super) fn validate(version: i64, file: &str, sql: &str) -> Result<(), String> {
    if version <= 211 {
        return Ok(());
    }
    validate_sql(sql).map_err(|error| format!("primary-key guard: {file}: {error}"))
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Word(String),
    Identifier(String),
    Literal,
    Symbol(u8),
}

impl Token {
    fn keyword(&self, expected: &str) -> bool {
        matches!(self, Self::Word(word) if word.eq_ignore_ascii_case(expected))
    }

    fn identifier(&self) -> Result<String, String> {
        match self {
            Self::Word(word) => Ok(word.to_ascii_lowercase()),
            Self::Identifier(word) => Ok(word.clone()),
            _ => Err("expected a column or table identifier".into()),
        }
    }
}

fn pair(tokens: &[Token], first: &str, second: &str) -> bool {
    tokens.len() >= 2 && tokens[0].keyword(first) && tokens[1].keyword(second)
}

fn validate_sql(sql: &str) -> Result<(), String> {
    let tokens = tokenize(sql)?;
    let mut i = 0;
    while i < tokens.len() {
        if pair(&tokens[i..], "PRIMARY", "KEY") {
            return Err("define primary keys together with explicit NOT NULL columns in CREATE TABLE; ALTER primary-key definitions require extending this guard".into());
        }
        if !tokens[i].keyword("CREATE") {
            i += 1;
            continue;
        }
        i += 1;
        if tokens
            .get(i)
            .is_some_and(|t| t.keyword("TEMP") || t.keyword("TEMPORARY") || t.keyword("UNLOGGED"))
        {
            i += 1;
        }
        // Virtual tables (e.g. FTS) have module-defined schemas, not ordinary
        // SQL primary keys. They remain subject to their engine integration tests.
        if !tokens.get(i).is_some_and(|t| t.keyword("TABLE")) {
            continue;
        }
        i += 1;
        if tokens.get(i).is_some_and(|t| t.keyword("IF")) {
            if !pair(&tokens[i + 1..], "NOT", "EXISTS") {
                return Err("expected IF NOT EXISTS".into());
            }
            i += 3;
        }
        let mut table = tokens.get(i).ok_or("missing table name")?.identifier()?;
        i += 1;
        while tokens.get(i) == Some(&Token::Symbol(b'.')) {
            table.push('.');
            table.push_str(
                &tokens
                    .get(i + 1)
                    .ok_or("missing table name")?
                    .identifier()?,
            );
            i += 2;
        }
        if tokens.get(i) != Some(&Token::Symbol(b'(')) {
            return Err(format!(
                "{table}: use an explicit column list and primary key; CREATE TABLE AS/LIKE is not checked"
            ));
        }
        let (definitions, next) = parenthesized_parts(&tokens, i)?;
        validate_table(&table, definitions)?;
        i = next;
    }
    Ok(())
}

fn parenthesized_parts(tokens: &[Token], open: usize) -> Result<(Vec<&[Token]>, usize), String> {
    let mut depth = 0;
    let mut start = open + 1;
    let mut parts = Vec::new();
    for (i, token) in tokens.iter().enumerate().skip(open + 1) {
        match token {
            Token::Symbol(b'(') => depth += 1,
            Token::Symbol(b')') if depth == 0 => {
                parts.push(&tokens[start..i]);
                return Ok((parts, i + 1));
            }
            Token::Symbol(b')') => depth -= 1,
            Token::Symbol(b',') if depth == 0 => {
                parts.push(&tokens[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    Err("unterminated table or primary-key definition".into())
}

fn validate_table(table: &str, definitions: Vec<&[Token]>) -> Result<(), String> {
    let mut columns = BTreeMap::new();
    let mut primary_key = Vec::new();
    for mut definition in definitions {
        if definition.first().is_some_and(|t| t.keyword("CONSTRAINT")) {
            definition = definition.get(2..).ok_or("missing named constraint")?;
        }
        if pair(definition, "PRIMARY", "KEY") {
            if definition.get(2) != Some(&Token::Symbol(b'(')) {
                return Err(format!("{table}: expected primary-key column list"));
            }
            let (parts, _) = parenthesized_parts(definition, 2)?;
            for part in parts {
                primary_key.push(part.first().ok_or("empty primary key")?.identifier()?);
            }
        } else if definition
            .first()
            .is_some_and(|t| t.keyword("UNIQUE") || t.keyword("CHECK") || t.keyword("FOREIGN"))
        {
            continue;
        } else {
            let name = definition
                .first()
                .ok_or("empty column definition")?
                .identifier()?;
            // CHECK/default expressions cannot impersonate a column constraint.
            let mut depth = 0;
            let mut not_null = false;
            for (i, token) in definition.iter().enumerate().skip(1) {
                match token {
                    Token::Symbol(b'(') => depth += 1,
                    Token::Symbol(b')') => depth -= 1,
                    _ if depth == 0 => {
                        not_null |= pair(&definition[i..], "NOT", "NULL");
                        if pair(&definition[i..], "PRIMARY", "KEY") {
                            primary_key.push(name.clone());
                        }
                    }
                    _ => {}
                }
            }
            columns.insert(name, not_null);
        }
    }
    if primary_key.is_empty() {
        return Err(format!(
            "{table}: ordinary tables must declare a PRIMARY KEY"
        ));
    }
    for column in primary_key {
        if columns.get(&column) != Some(&true) {
            return Err(format!(
                "{table}.{column}: primary-key columns must declare explicit NOT NULL in both SQLite and PostgreSQL"
            ));
        }
    }
    Ok(())
}

// A lexer, not a SQL execution/parser substitute. Quoted data and comments must
// never count as constraints; commas inside expressions must not split columns.
fn tokenize(sql: &str) -> Result<Vec<Token>, String> {
    let bytes = sql.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let start = i;
        match bytes[i] {
            b if b.is_ascii_whitespace() => i += 1,
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i += 2;
                let mut depth = 1;
                while i + 1 < bytes.len() && depth > 0 {
                    match &bytes[i..i + 2] {
                        b"/*" => {
                            depth += 1;
                            i += 2;
                        }
                        b"*/" => {
                            depth -= 1;
                            i += 2;
                        }
                        _ => i += 1,
                    }
                }
                if depth != 0 {
                    return Err("unterminated SQL comment".into());
                }
            }
            quote @ (b'\'' | b'"' | b'`' | b'[') => {
                let close = if quote == b'[' { b']' } else { quote };
                let escaped =
                    quote == b'\'' && start > 0 && matches!(bytes[start - 1], b'e' | b'E');
                i += 1;
                let mut value = Vec::new();
                loop {
                    let byte = *bytes.get(i).ok_or("unterminated SQL quote")?;
                    i += 1;
                    if byte == close {
                        if close != b']' && bytes.get(i) == Some(&close) {
                            i += 1;
                        } else {
                            break;
                        }
                    } else if escaped && byte == b'\\' {
                        i += 1;
                    }
                    value.push(byte);
                }
                tokens.push(if quote == b'\'' {
                    Token::Literal
                } else {
                    Token::Identifier(String::from_utf8(value).map_err(|e| e.to_string())?)
                });
            }
            b'$' => {
                let mut end = i + 1;
                while bytes
                    .get(end)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
                {
                    end += 1;
                }
                if bytes.get(end) == Some(&b'$') {
                    let delimiter = &sql[i..=end];
                    let length = sql[end + 1..]
                        .find(delimiter)
                        .ok_or("unterminated dollar-quoted SQL")?;
                    i = end + 1 + length + delimiter.len();
                    tokens.push(Token::Literal);
                } else {
                    tokens.push(Token::Symbol(b'$'));
                    i += 1;
                }
            }
            b if b.is_ascii_alphanumeric() || b == b'_' || b >= 128 => {
                i += 1;
                while bytes
                    .get(i)
                    .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b >= 128)
                {
                    i += 1;
                }
                tokens.push(Token::Word(sql[start..i].to_string()));
            }
            byte => {
                tokens.push(Token::Symbol(byte));
                i += 1;
            }
        }
    }
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_key_guard_rejects_nullable_inline_and_composite_keys() {
        for sql in [
            "CREATE TABLE items (id TEXT PRIMARY KEY);",
            "CREATE TABLE items (id BIGINT PRIMARY KEY CHECK (id = 1));",
            "CREATE TABLE items (id INTEGER PRIMARY KEY);",
            "CREATE TABLE items (a TEXT NOT NULL, b TEXT, PRIMARY KEY (a, b));",
            "CREATE TABLE items (a TEXT NOT NULL, b INTEGER, CONSTRAINT identity PRIMARY KEY (a, b));",
            "CREATE TABLE items (id TEXT PRIMARY KEY /* NOT NULL */);",
            "CREATE TABLE items (id TEXT PRIMARY KEY CHECK (id IS NOT NULL));",
            "CREATE TABLE items (id TEXT PRIMARY KEY, value TEXT NOT NULL);",
            "CREATE TABLE items (id TEXT PRIMARY KEY DEFAULT 'NOT NULL');",
            "CREATE TABLE items (id TEXT PRIMARY KEY DEFAULT $$NOT NULL$$);",
            "CREATE TABLE items (id TEXT PRIMARY KEY CONSTRAINT \"NOT NULL\" UNIQUE);",
            "CREATE TABLE items (id TEXT PRIMARY KEY) WITHOUT ROWID;",
            "CREATE TABLE items (id TEXT PRIMARY KEY) STRICT;",
        ] {
            let error = validate(212, "migrations/example.sql", sql).unwrap_err();
            assert!(error.contains("explicit NOT NULL"), "{sql}: {error}");
            assert!(error.contains("migrations/example.sql"));
        }
    }

    #[test]
    fn primary_key_guard_handles_sql_quoting_comments_and_nested_expressions() {
        for sql in [
            "CREATE TABLE items (id TEXT PRIMARY KEY NOT NULL);",
            "cReAtE TABLE IF NOT EXISTS main.\"items\" (\"a\" TEXT NOT NULL, b BIGINT NOT NULL, CONSTRAINT pk PRIMARY KEY (\"a\" DESC, b));",
            "CREATE TEMPORARY TABLE [items] ([id] INTEGER NOT NULL PRIMARY KEY);",
            "CREATE UNLOGGED TABLE items (id TEXT NOT NULL PRIMARY KEY, x NUMERIC(10, 2) DEFAULT coalesce(1, 2));",
            "/* CREATE TABLE bad (id TEXT PRIMARY KEY); /* nested */ */ CREATE TABLE `items` (`id` TEXT NOT /* gap */ NULL PRIMARY KEY);",
            "SELECT 'CREATE TABLE bad (id TEXT PRIMARY KEY)'; -- CREATE TABLE bad (id TEXT PRIMARY KEY)\n SELECT $tag$CREATE TABLE bad (id TEXT PRIMARY KEY)$tag$;",
            "CREATE TABLE items (\"a\"\"b\" TEXT NOT NULL, PRIMARY KEY (\"a\"\"b\"));",
            "CREATE VIRTUAL TABLE title_search_fts USING fts5(name);",
            "ALTER TABLE proxy_configs ALTER COLUMN protocol DROP NOT NULL;",
        ] {
            validate(212, "test.sql", sql).unwrap_or_else(|error| panic!("{sql}: {error}"));
        }
    }

    #[test]
    fn primary_key_guard_requires_declared_keys_and_checks_every_table() {
        for sql in [
            "CREATE TABLE items (id TEXT NOT NULL UNIQUE);",
            "CREATE TABLE items AS SELECT 1 AS id;",
            "CREATE TABLE items (LIKE old_items INCLUDING ALL);",
            "CREATE TABLE good (id TEXT NOT NULL PRIMARY KEY); CREATE TABLE bad (id TEXT PRIMARY KEY);",
            "CREATE TABLE items (id TEXT NOT NULL, PRIMARY KEY (missing));",
            "CREATE TABLE items (\"ID\" TEXT, id TEXT NOT NULL, PRIMARY KEY (\"ID\"));",
            "ALTER TABLE items ADD CONSTRAINT pk PRIMARY KEY (id);",
            "CREATE TABLE items (id TEXT NOT NULL PRIMARY KEY",
            "/* unterminated",
        ] {
            assert!(validate(212, "test.sql", sql).is_err(), "{sql}");
        }
    }

    #[test]
    fn primary_key_guard_grandfathers_only_the_fixed_released_boundary() {
        let sql = "CREATE TABLE items (id TEXT PRIMARY KEY);";
        assert!(validate(211, "released.sql", sql).is_ok());
        for version in [212, 239, 999] {
            for file in [
                "migrations/new.sql",
                "postgres/migrations/new.sql",
                "baselines/new.sql",
            ] {
                assert!(validate(version, file, sql).is_err());
            }
        }
    }
}
