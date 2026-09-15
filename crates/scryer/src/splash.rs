use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use tokio::sync::watch;
use tower::ServiceExt;
use tower_http::compression::CompressionLayer;

use crate::base_path::{BasePath, mount_router};
use crate::middleware::{CorsConfig, cors_handler};

#[derive(Clone)]
pub(crate) enum BootstrapStatus {
    Migrating,
    Ready(Router),
    Failed(String),
}

#[derive(Clone)]
pub(crate) struct SplashState {
    pub(crate) status_rx: watch::Receiver<BootstrapStatus>,
}

/// `GET /health` — the health check for every orchestrator probe (liveness,
/// readiness, startup, reverse-proxy upstream checks).
///
/// Reports HTTP 200 for the whole time the process is alive and making
/// progress — including while migrations run — so Docker/Compose healthchecks,
/// Kubernetes probes, autoheal, etc. never recycle the container in the middle
/// of a long migration. User traffic does not need to be held back during that
/// window either: the splash fallback serves the "Upgrading database…" page on
/// every route and it reloads into the app the moment bootstrap finishes, so a
/// readiness probe here is correct too. The JSON body carries the bootstrap
/// phase (`"migrating"` / `"ok"`) plus a `ready` flag, and every in-tree poller
/// (splash page, web client restart overlay, xtask, seed) keys on
/// `status == "ok"`, not on the HTTP status. Only a failed bootstrap returns a
/// non-2xx code, because a process that will never become ready *should* be
/// recycled.
///
/// Automation that needs the *API* up (not just the process) uses
/// `GET /health/ready` (see [`splash_ready_handler`]).
pub(crate) async fn splash_health_handler(State(state): State<SplashState>) -> Response {
    let status = state.status_rx.borrow().clone();
    match status {
        BootstrapStatus::Migrating => {
            Json(serde_json::json!({"status": "migrating", "ready": false})).into_response()
        }
        BootstrapStatus::Ready(_) => {
            Json(serde_json::json!({"status": "ok", "ready": true})).into_response()
        }
        BootstrapStatus::Failed(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"status": "error", "ready": false, "message": message})),
        )
            .into_response(),
    }
}

/// `GET /health/ready` — "is the application (GraphQL/UI) serving yet?"
///
/// Returns 503 until the full application router is serving, 200 afterwards,
/// and 500 if bootstrap failed. This is for automation that must not call the
/// API before bootstrap completes (CI, seed/provisioning scripts, e2e, boot-time
/// sidecars) and wants the HTTP status code rather than the JSON body to say
/// so. It is *not* the orchestrator health check — a restart-on-unhealthy or
/// `startupProbe` pointed here would kill a long migration; those belong on
/// `/health`.
pub(crate) async fn splash_ready_handler(State(state): State<SplashState>) -> Response {
    let status = state.status_rx.borrow().clone();
    match status {
        BootstrapStatus::Migrating => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"status": "migrating", "ready": false})),
        )
            .into_response(),
        BootstrapStatus::Ready(_) => {
            Json(serde_json::json!({"status": "ok", "ready": true})).into_response()
        }
        BootstrapStatus::Failed(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"status": "error", "ready": false, "message": message})),
        )
            .into_response(),
    }
}

/// Where the splash page loads the turning Scryer mark from. The splash router
/// answers every other path with the splash page itself, so the artwork needs a
/// route of its own, and the web UI's assets are not being served yet.
const SPLASH_LOADING_MARK_PATH: &str = "/splash/scryer-loading.webp";
const SPLASH_LOADING_MARK_STILL_PATH: &str = "/splash/scryer-loading-still.webp";

/// The full-size animated mark and its first frame, for reduced motion.
static SPLASH_LOADING_MARK: &[u8] = include_bytes!("../resources/splash/scryer-loading.webp");
static SPLASH_LOADING_MARK_STILL: &[u8] =
    include_bytes!("../resources/splash/scryer-loading-still.webp");

fn webp_response(bytes: &'static [u8]) -> Response {
    (
        [
            (header::CONTENT_TYPE, "image/webp"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        bytes,
    )
        .into_response()
}

async fn splash_loading_mark_handler() -> Response {
    webp_response(SPLASH_LOADING_MARK)
}

async fn splash_loading_mark_still_handler() -> Response {
    webp_response(SPLASH_LOADING_MARK_STILL)
}

pub(crate) async fn splash_fallback_handler(
    State(state): State<SplashState>,
    request: axum::extract::Request,
) -> Response {
    let status = state.status_rx.borrow().clone();
    match status {
        BootstrapStatus::Ready(router) => router
            .oneshot(request)
            .await
            .unwrap_or_else(|err| match err {}),
        BootstrapStatus::Migrating => Html(splash_html()).into_response(),
        BootstrapStatus::Failed(message) => Html(error_html(&message)).into_response(),
    }
}

pub(crate) fn build_splash_router(
    state: SplashState,
    cors: CorsConfig,
    base_path: BasePath,
) -> Router {
    let cors_for_layer = cors.clone();

    let router = Router::new()
        .route(
            "/health",
            get(splash_health_handler).with_state(state.clone()),
        )
        .route(
            "/health/ready",
            get(splash_ready_handler).with_state(state.clone()),
        )
        .route(SPLASH_LOADING_MARK_PATH, get(splash_loading_mark_handler))
        .route(
            SPLASH_LOADING_MARK_STILL_PATH,
            get(splash_loading_mark_still_handler),
        )
        .fallback(splash_fallback_handler)
        .with_state(state)
        .layer(CompressionLayer::new().zstd(true).br(true).gzip(true))
        .layer(axum::middleware::from_fn(move |request, next| {
            cors_handler(request, next, cors_for_layer.clone())
        }));

    mount_router(router, &base_path)
}

fn splash_html() -> String {
    let base_path = BasePath::from_env();
    let health_url = base_path.join("/health");
    let loading_mark_url = base_path.join(SPLASH_LOADING_MARK_PATH);
    let loading_mark_still_url = base_path.join(SPLASH_LOADING_MARK_STILL_PATH);
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>scryer — upgrading</title>
<style>{SPLASH_STYLE}</style>
</head>
<body>
<main>
  <h1>scryer</h1>
  <picture class="loading-mark">
    <source media="(prefers-reduced-motion: reduce)" srcset="{loading_mark_still_url}"/>
    <img src="{loading_mark_url}" width="531" height="522" alt=""/>
  </picture>
  <div class="status">Upgrading database&hellip;</div>
</main>
<script>
(function() {{
  var delay = 200;
  function poll() {{
    fetch("{health_url}")
      .then(function(r) {{ return r.json(); }})
      .then(function(d) {{
        if (d.status === "ok") location.reload();
        else if (d.status === "error") {{
          document.querySelector(".loading-mark").style.display = "none";
          var s = document.querySelector(".status");
          s.textContent = "Startup failed";
          s.classList.add("error");
          var p = document.createElement("p");
          p.className = "detail";
          p.textContent = d.message || "Unknown error";
          document.querySelector("main").appendChild(p);
          return;
        }}
      }})
      .catch(function() {{}})
      .finally(function() {{ delay = 1000; setTimeout(poll, delay); }});
  }}
  setTimeout(poll, delay);
}})();
</script>
</body>
</html>"#
    )
}

fn error_html(message: &str) -> String {
    let escaped = message
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;");
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8"/>
<meta name="viewport" content="width=device-width,initial-scale=1"/>
<title>scryer — error</title>
<style>{SPLASH_STYLE}</style>
</head>
<body>
<main>
  <h1>scryer</h1>
  <div class="status error">Startup failed</div>
  <p class="detail">{escaped}</p>
</main>
</body>
</html>"#
    )
}

const SPLASH_STYLE: &str = r#"
:root { color-scheme: dark; }
* { margin: 0; padding: 0; box-sizing: border-box; }
body {
  min-height: 100vh;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, sans-serif;
  background: #070b18;
  color: #dbe5ff;
  display: grid;
  place-items: center;
}
main {
  text-align: center;
  padding: 2rem;
}
h1 {
  font-family: "Space Grotesk", Inter, ui-sans-serif, system-ui, sans-serif;
  font-size: 2rem;
  font-weight: 700;
  letter-spacing: -0.02em;
  margin-bottom: 2rem;
  color: #dbe5ff;
}
.status {
  font-size: 0.95rem;
  color: #8b96b9;
  margin-bottom: 0.5rem;
}
.status.error {
  color: #ef4444;
  font-weight: 600;
}
.detail {
  font-size: 0.85rem;
  color: #8b96b9;
  max-width: 36rem;
  margin: 1rem auto 0;
  word-break: break-word;
}
.loading-mark img {
  display: block;
  width: 160px;
  max-width: 50vw;
  height: auto;
  margin: 0 auto 1.5rem;
}
"#;

#[cfg(test)]
mod tests {
    use super::{BootstrapStatus, SplashState, build_splash_router};
    use crate::base_path::BasePath;
    use crate::middleware::CorsConfig;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use axum::routing::get;
    use tokio::sync::watch;
    use tower::ServiceExt;

    fn ready_splash_router(inner: Router) -> Router {
        splash_router_for(BootstrapStatus::Ready(inner))
    }

    fn splash_router_for(status: BootstrapStatus) -> Router {
        // Dropping the sender is fine: `borrow()` keeps returning the last value.
        let (_status_tx, status_rx) = watch::channel(status);
        build_splash_router(
            SplashState { status_rx },
            CorsConfig {
                allow_all: false,
                allowed_origins: vec![],
            },
            BasePath::from_raw(Some("/scryer/")),
        )
    }

    async fn get_json(app: Router, uri: &str) -> (StatusCode, serde_json::Value) {
        let response = app
            .oneshot(
                Request::builder()
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .expect("body");
        let payload = serde_json::from_slice(&bytes).expect("json body");
        (status, payload)
    }

    #[tokio::test]
    async fn liveness_reports_ok_while_migrating_but_flags_not_ready() {
        // Orchestrator liveness probes must not recycle the process during a
        // long migration: HTTP 200, with the phase carried in the body.
        let (status, payload) = get_json(
            splash_router_for(BootstrapStatus::Migrating),
            "/scryer/health",
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["status"], "migrating");
        assert_eq!(payload["ready"], false);
    }

    #[tokio::test]
    async fn readiness_is_unavailable_while_migrating() {
        let (status, payload) = get_json(
            splash_router_for(BootstrapStatus::Migrating),
            "/scryer/health/ready",
        )
        .await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(payload["status"], "migrating");
        assert_eq!(payload["ready"], false);
    }

    #[tokio::test]
    async fn liveness_and_readiness_report_ok_once_ready() {
        let inner = || Router::new().route("/", get(|| async { StatusCode::OK }));
        let (status, payload) = get_json(ready_splash_router(inner()), "/scryer/health").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["ready"], true);

        let (status, payload) =
            get_json(ready_splash_router(inner()), "/scryer/health/ready").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(payload["status"], "ok");
        assert_eq!(payload["ready"], true);
    }

    #[tokio::test]
    async fn failed_bootstrap_reports_error_on_both_probes() {
        // A process that will never become ready should be recycled, so both
        // probes go non-2xx and carry the failure message.
        for path in ["/scryer/health", "/scryer/health/ready"] {
            let (status, payload) = get_json(
                splash_router_for(BootstrapStatus::Failed("boom".to_string())),
                path,
            )
            .await;
            assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{path}");
            assert_eq!(payload["status"], "error", "{path}");
            assert_eq!(payload["ready"], false, "{path}");
            assert_eq!(payload["message"], "boom", "{path}");
        }
    }

    #[tokio::test]
    async fn splash_serves_the_loading_mark_while_migrating() {
        // Every other path answers with the splash page, so the page's artwork
        // needs its own route, under the base path like the page's health poll.
        for (path, expected) in [
            (
                "/scryer/splash/scryer-loading.webp",
                super::SPLASH_LOADING_MARK,
            ),
            (
                "/scryer/splash/scryer-loading-still.webp",
                super::SPLASH_LOADING_MARK_STILL,
            ),
        ] {
            let response = splash_router_for(BootstrapStatus::Migrating)
                .oneshot(
                    Request::builder()
                        .uri(path)
                        .header("accept-encoding", "br, gzip")
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(response.headers()["content-type"], "image/webp", "{path}");
            let bytes = axum::body::to_bytes(response.into_body(), 4 * 1024 * 1024)
                .await
                .expect("body");
            assert_eq!(&bytes[..], expected, "{path}");
        }
    }

    #[tokio::test]
    async fn health_probes_answer_head_requests_like_get() {
        // `wget --spider` and `curl -I` style orchestrator probes send HEAD;
        // they must see the same status codes as GET on both endpoints.
        let cases = [
            (BootstrapStatus::Migrating, "/scryer/health", StatusCode::OK),
            (
                BootstrapStatus::Migrating,
                "/scryer/health/ready",
                StatusCode::SERVICE_UNAVAILABLE,
            ),
            (
                BootstrapStatus::Ready(Router::new()),
                "/scryer/health",
                StatusCode::OK,
            ),
            (
                BootstrapStatus::Ready(Router::new()),
                "/scryer/health/ready",
                StatusCode::OK,
            ),
        ];
        for (state, path, expected) in cases {
            let response = splash_router_for(state)
                .oneshot(
                    Request::builder()
                        .method(Method::HEAD)
                        .uri(path)
                        .body(Body::empty())
                        .expect("request"),
                )
                .await
                .expect("response");
            assert_eq!(response.status(), expected, "HEAD {path}");
            let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
                .await
                .expect("body");
            assert!(bytes.is_empty(), "HEAD {path} must not carry a body");
        }
    }

    #[tokio::test]
    async fn prefixed_ready_router_serves_ui_root_without_redirect_loop() {
        let app = ready_splash_router(Router::new().route("/", get(|| async { StatusCode::OK })));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/scryer/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn prefixed_ready_router_serves_subpaths() {
        let app =
            ready_splash_router(Router::new().route("/login", get(|| async { StatusCode::OK })));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/scryer/login")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn prefixed_splash_router_does_not_handle_root() {
        let app = ready_splash_router(Router::new().route("/", get(|| async { StatusCode::OK })));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        // Root `/` should not be handled when a base path is configured — another
        // service may live at `/` behind the same reverse proxy.
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn prefixed_splash_health_uses_base_path() {
        let app = ready_splash_router(Router::new().route("/", get(|| async { StatusCode::OK })));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/scryer/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
    }
}
