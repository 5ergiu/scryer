use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::Value;
use std::sync::Arc;

use crate::ScenarioState;
use crate::fixtures::load_fixture;

/// Build the SMG (Scryer Metadata Gateway) GraphQL mock router.
///
/// Handles:
/// - GET `/graphql` — APQ (Automatic Persisted Query) cache hit path
/// - POST `/graphql` — full query fallback path
pub fn router() -> Router<Arc<ScenarioState>> {
    Router::new().route(
        "/graphql",
        get(graphql_get_handler).post(graphql_post_handler),
    )
}

/// APQ GET handler — parses the extensions to determine the query type.
async fn graphql_get_handler(
    State(state): State<Arc<ScenarioState>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Value> {
    let scenario = state.current_scenario();
    let operation_name = params
        .get("operationName")
        .map(String::as_str)
        .unwrap_or_default();
    tracing::debug!(scenario = %scenario, "smg graphql GET (APQ)");

    fixture_response(operation_name, "")
}

/// POST handler — parses the query body to determine response.
async fn graphql_post_handler(
    State(state): State<Arc<ScenarioState>>,
    Json(body): Json<Value>,
) -> Json<Value> {
    let scenario = state.current_scenario();
    let query = body.get("query").and_then(Value::as_str).unwrap_or("");
    let operation_name = body
        .get("operationName")
        .and_then(Value::as_str)
        .unwrap_or_default();

    tracing::debug!(scenario = %scenario, query_len = query.len(), "smg graphql POST");

    fixture_response(operation_name, query)
}

fn fixture_response(operation_name: &str, query: &str) -> Json<Value> {
    let operation = if query.is_empty() {
        operation_name
    } else {
        query
    };
    let fixture =
        if operation.contains("searchTitlesBatch") || operation.contains("SearchTitlesBatch") {
            "smg/search_titles_batch.json"
        } else if operation.contains("searchTitles(") || operation.contains("SearchTitles") {
            "smg/search_titles.json"
        } else if operation.contains("resolveTitles(") || operation.contains("ResolveTitles") {
            "smg/resolve_titles.json"
        } else if operation.contains("titles(") || operation.contains("Titles") {
            "smg/titles_movie.json"
        } else if operation.contains("metadataBulk(") || operation.contains("MetadataBulk") {
            "smg/metadata_bulk_movie.json"
        } else if operation.contains("searchTvdbBatch") || operation.contains("SearchTvdbBatch") {
            "smg/search_tvdb_batch.json"
        } else if operation.contains("searchTvdbMulti") || operation.contains("SearchTvdbMulti") {
            "smg/search_tvdb_multi.json"
        } else if operation.contains("series(") || operation.contains("GetSeries") {
            "smg/get_series.json"
        } else if operation.contains("movie(") || operation.contains("GetMovie") {
            "smg/get_movie.json"
        } else {
            "smg/search_tvdb_rich.json"
        };
    let fixture = load_fixture(fixture);
    let mut parsed: Value = serde_json::from_str(&fixture).expect("valid fixture");
    if !query.is_empty() && !query.contains("TMDB_PRIMARY_SERIES") {
        withhold_tmdb_primary_series(&mut parsed);
    }
    Json(parsed)
}

/// SMG serves TMDB-primary series (`tvdb_id: null`) only to documents that
/// declare `clientCapabilities: [TMDB_PRIMARY_SERIES]`. A POSTed document
/// carries its text, so the mock can honour that; an APQ GET carries only the
/// hash, which the mock treats as Scryer's own (capable) documents.
fn withhold_tmdb_primary_series(response: &mut Value) {
    if let Some(series) = response
        .pointer_mut("/data/titles/series")
        .and_then(Value::as_array_mut)
    {
        series.retain(|item| !item["tvdb_id"].is_null());
    }
}

#[cfg(test)]
mod tests {
    use super::fixture_response;

    #[test]
    fn routes_title_id_operations_by_operation_name() {
        let response = fixture_response("Titles", "");
        assert!(response["data"]["titles"].is_object());

        let response = fixture_response(
            "",
            "query { searchTitles(query: \"x\") { results { title_id } } }",
        );
        assert!(response["data"]["searchTitles"].is_object());
    }

    #[test]
    fn serves_tmdb_primary_series_only_to_capable_documents() {
        let tmdb_primary_series = |response: &serde_json::Value| {
            response["data"]["titles"]["series"]
                .as_array()
                .map(|series| {
                    series
                        .iter()
                        .filter(|item| item["tvdb_id"].is_null())
                        .count()
                })
                .unwrap_or_default()
        };

        assert_eq!(tmdb_primary_series(&fixture_response("Titles", "")), 1);
        assert_eq!(
            tmdb_primary_series(&fixture_response(
                "Titles",
                "query Titles { titles(ids: [303], clientCapabilities: [TMDB_PRIMARY_SERIES]) { series { id } } }",
            )),
            1
        );
        assert_eq!(
            tmdb_primary_series(&fixture_response(
                "Titles",
                "query Titles { titles(ids: [303]) { series { id } } }",
            )),
            0
        );
    }
}
