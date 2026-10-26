//! The per-cycle subtitle provider circuit breaker: a provider that keeps
//! failing sits out the rest of a subtitle search cycle while the healthy ones
//! carry on, and the next cycle gives it a single try.

use super::*;

use std::sync::atomic::{AtomicUsize, Ordering};

use crate::subtitles::provider::{SubtitleFile, SubtitleQuery};
use scryer_domain::{PluginHostBindingId, SubtitleProviderConfig};

const FAILING_PROVIDER: &str = "brokensubs";
const HEALTHY_PROVIDER: &str = "steadysubs";

/// A provider whose every search fails the way a hard plugin error does.
struct FailingSubtitleClient {
    searches: Arc<AtomicUsize>,
}

#[async_trait]
impl crate::SubtitleProviderClient for FailingSubtitleClient {
    async fn search(
        &self,
        _query: &SubtitleQuery,
    ) -> AppResult<Vec<crate::subtitles::SubtitleMatch>> {
        self.searches.fetch_add(1, Ordering::SeqCst);
        Err(AppError::plugin_failed(
            &scryer_plugin_sdk::PluginError {
                code: scryer_plugin_sdk::PluginErrorCode::Permanent,
                public_message: "synthetic provider failure".to_string(),
                debug_message: None,
                retry_after_seconds: None,
                details: None,
            },
            "subtitle_search",
        ))
    }

    async fn download(&self, _provider_file_id: &str) -> AppResult<SubtitleFile> {
        Err(AppError::Repository(
            "the failing provider never offers a download".to_string(),
        ))
    }

    async fn validate_connection(&self) -> AppResult<crate::SubtitleProviderValidationResult> {
        Err(AppError::Repository(
            "validation is not used here".to_string(),
        ))
    }

    fn name(&self) -> &str {
        FAILING_PROVIDER
    }
}

/// A provider that answers every search, with one match or with none.
struct HealthySubtitleClient {
    offers_match: bool,
    searches: Arc<AtomicUsize>,
    downloads: Arc<AtomicUsize>,
}

#[async_trait]
impl crate::SubtitleProviderClient for HealthySubtitleClient {
    async fn search(
        &self,
        query: &SubtitleQuery,
    ) -> AppResult<Vec<crate::subtitles::SubtitleMatch>> {
        let call = self.searches.fetch_add(1, Ordering::SeqCst);
        if !self.offers_match {
            return Ok(Vec::new());
        }
        Ok(vec![crate::subtitles::SubtitleMatch {
            provider: HEALTHY_PROVIDER.to_string(),
            provider_file_id: format!("steady-file-{call}"),
            language: query
                .languages
                .first()
                .cloned()
                .unwrap_or_else(|| "eng".to_string()),
            release_info: Some("Synthetic.Release.Group".to_string()),
            score: 90,
            score_percent: 90,
            hearing_impaired: false,
            forced: false,
            ai_translated: false,
            machine_translated: false,
            uploader: None,
            download_count: None,
            hash_matched: false,
        }])
    }

    async fn download(&self, _provider_file_id: &str) -> AppResult<SubtitleFile> {
        self.downloads.fetch_add(1, Ordering::SeqCst);
        Ok(SubtitleFile {
            content: b"1\n00:00:01,000 --> 00:00:02,000\nline\n".to_vec(),
            format: "srt".to_string(),
            filename: None,
            content_type: None,
        })
    }

    async fn validate_connection(&self) -> AppResult<crate::SubtitleProviderValidationResult> {
        Err(AppError::Repository(
            "validation is not used here".to_string(),
        ))
    }

    fn name(&self) -> &str {
        HEALTHY_PROVIDER
    }
}

#[derive(Clone, Default)]
struct ProviderCalls {
    failing_searches: Arc<AtomicUsize>,
    healthy_searches: Arc<AtomicUsize>,
    healthy_downloads: Arc<AtomicUsize>,
}

struct CircuitPluginProvider {
    healthy_offers_match: bool,
    calls: ProviderCalls,
}

impl crate::SubtitlePluginProvider for CircuitPluginProvider {
    fn client_for_config(
        &self,
        config: &SubtitleProviderConfig,
        _host_bindings: &HashMap<PluginHostBindingId, String>,
    ) -> Option<Arc<dyn crate::SubtitleProviderClient>> {
        match config.provider_type.as_str() {
            FAILING_PROVIDER => Some(Arc::new(FailingSubtitleClient {
                searches: Arc::clone(&self.calls.failing_searches),
            })),
            HEALTHY_PROVIDER => Some(Arc::new(HealthySubtitleClient {
                offers_match: self.healthy_offers_match,
                searches: Arc::clone(&self.calls.healthy_searches),
                downloads: Arc::clone(&self.calls.healthy_downloads),
            })),
            _ => None,
        }
    }

    fn available_provider_types(&self) -> Vec<String> {
        vec![FAILING_PROVIDER.to_string(), HEALTHY_PROVIDER.to_string()]
    }

    fn supports_catalog_search_for_provider(&self, provider_type: &str) -> bool {
        [FAILING_PROVIDER, HEALTHY_PROVIDER].contains(&provider_type)
    }

    fn recommended_facets_for_provider(&self, _provider_type: &str) -> Vec<String> {
        vec!["movie".to_string()]
    }

    fn config_fields_for_provider(
        &self,
        _provider_type: &str,
    ) -> Vec<scryer_domain::ConfigFieldDef> {
        Vec::new()
    }

    fn plugin_name_for_provider(&self, provider_type: &str) -> Option<String> {
        Some(provider_type.to_string())
    }
}

struct CircuitProviderConfigs {
    configs: Vec<SubtitleProviderConfig>,
}

#[async_trait]
impl crate::SubtitleProviderConfigRepository for CircuitProviderConfigs {
    async fn list(&self, provider_type: Option<String>) -> AppResult<Vec<SubtitleProviderConfig>> {
        Ok(self
            .configs
            .iter()
            .filter(|config| {
                provider_type
                    .as_deref()
                    .is_none_or(|wanted| config.provider_type == wanted)
            })
            .cloned()
            .collect())
    }

    async fn get_by_id(&self, id: &str) -> AppResult<Option<SubtitleProviderConfig>> {
        Ok(self.configs.iter().find(|config| config.id == id).cloned())
    }

    async fn create(&self, config: SubtitleProviderConfig) -> AppResult<SubtitleProviderConfig> {
        Ok(config)
    }

    async fn update(
        &self,
        update: crate::SubtitleProviderConfigUpdate,
    ) -> AppResult<SubtitleProviderConfig> {
        self.get_by_id(&update.id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("subtitle provider config {}", update.id)))
    }

    async fn delete(&self, _id: &str) -> AppResult<()> {
        Ok(())
    }
}

fn provider_config(provider_type: &str) -> SubtitleProviderConfig {
    let now = Utc::now();
    SubtitleProviderConfig {
        id: format!("{provider_type}-config"),
        name: provider_type.to_string(),
        provider_type: provider_type.to_string(),
        config_json: "{}".to_string(),
        enabled_facets: vec!["movie".to_string()],
        is_enabled: true,
        last_health_status: None,
        last_error: None,
        last_error_at: None,
        disabled_until: None,
        created_at: now,
        updated_at: now,
    }
}

/// `movies` monitored movies, each with one file and no subtitles, searched
/// for English by a failing provider and a healthy one.
async fn circuit_fixture(
    movies: usize,
    healthy_offers_match: bool,
) -> (AppUseCase, ProviderCalls, tempfile::TempDir) {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let movie_root = tempdir.path().join("movies");
    std::fs::create_dir_all(&movie_root).expect("create movie root");

    let media_files = Arc::new(MockMediaFileRepo::default());
    let (app, admin, _titles) = bootstrap_with_cutoff_projection_state(
        Arc::new(StoredSettingsRepo::default()),
        Arc::new(StoredQualityProfileRepo::default()),
        media_files,
    );
    let calls = ProviderCalls::default();
    let plugin_provider = Arc::new(CircuitPluginProvider {
        healthy_offers_match,
        calls: calls.clone(),
    });
    let app = app.with_test_overrides(|services| {
        services
            .with_subtitle_provider_configs(Arc::new(CircuitProviderConfigs {
                configs: vec![
                    provider_config(FAILING_PROVIDER),
                    provider_config(HEALTHY_PROVIDER),
                ],
            }))
            .with_subtitle_plugin_provider(plugin_provider)
    });

    app.update_media_settings(
        &admin,
        MediaFacet::Movie,
        empty_update_media_settings_with_roots(vec![build_root_folder_entry(&movie_root, true)]),
    )
    .await
    .expect("save roots");
    app.update_subtitle_settings(
        &admin,
        UpdateSubtitleSettings {
            enabled: true,
            languages: vec![crate::subtitles::wanted::SubtitleLanguagePref {
                code: "eng".to_string(),
                hearing_impaired: false,
                forced: false,
            }],
            auto_download_on_import: false,
            minimum_score_series: 90,
            minimum_score_movie: 70,
            search_interval_hours: 6,
            include_ai_translated: false,
            include_machine_translated: false,
            sync_enabled: false,
            sync_threshold_series: 90,
            sync_threshold_movie: 70,
            sync_max_offset_seconds: 60,
        },
    )
    .await
    .expect("save subtitle settings");

    for index in 0..movies {
        let name = format!("Quiet Meridian {index}");
        let title = app
            .add_title(
                &admin,
                NewTitle {
                    name: name.clone(),
                    facet: MediaFacet::Movie,
                    monitored: true,
                    tags: vec![],
                    external_ids: vec![],
                    min_availability: None,
                    ..Default::default()
                },
            )
            .await
            .expect("create title");
        let folder = movie_root.join(&name);
        std::fs::create_dir(&folder).expect("create title folder");
        app.services
            .catalog
            .titles
            .set_folder_path(&title.id, folder.to_str().unwrap())
            .await
            .expect("record title folder");
        let media_path = folder.join(format!("{name}.mkv"));
        std::fs::write(&media_path, b"video").expect("write media file");
        app.services
            .library
            .media_files
            .insert_media_file(&InsertMediaFileInput {
                title_id: title.id.clone(),
                file_path: media_path.to_string_lossy().to_string(),
                size_bytes: 5,
                role: MediaFileRole::Primary,
                ..Default::default()
            })
            .await
            .expect("insert media file");
    }

    (app, calls, tempdir)
}

#[tokio::test(start_paused = true)]
async fn subtitle_cycle_stops_calling_a_provider_after_three_failures_in_a_row() {
    let (app, calls, _tempdir) = circuit_fixture(5, false).await;

    app.run_subtitle_search_job()
        .await
        .expect("subtitle cycle runs");

    assert_eq!(
        calls.failing_searches.load(Ordering::SeqCst),
        3,
        "the failing provider is left out once its circuit opens"
    );
    assert_eq!(
        calls.healthy_searches.load(Ordering::SeqCst),
        5,
        "the healthy provider is searched for every title"
    );
}

#[tokio::test(start_paused = true)]
async fn the_next_subtitle_cycle_tries_a_tripped_provider_once() {
    let (app, calls, _tempdir) = circuit_fixture(5, false).await;
    app.run_subtitle_search_job()
        .await
        .expect("first subtitle cycle runs");
    assert_eq!(calls.failing_searches.load(Ordering::SeqCst), 3);

    app.run_subtitle_search_job()
        .await
        .expect("second subtitle cycle runs");

    assert_eq!(
        calls.failing_searches.load(Ordering::SeqCst),
        4,
        "one probe, and its failure opens the circuit again at once"
    );
    assert_eq!(calls.healthy_searches.load(Ordering::SeqCst), 10);
}

#[tokio::test(start_paused = true)]
async fn a_tripped_provider_does_not_stop_downloads_from_a_healthy_one() {
    let (app, calls, _tempdir) = circuit_fixture(4, true).await;

    app.run_subtitle_search_job()
        .await
        .expect("subtitle cycle runs");

    assert_eq!(calls.failing_searches.load(Ordering::SeqCst), 3);
    assert_eq!(calls.healthy_searches.load(Ordering::SeqCst), 4);
    assert_eq!(
        calls.healthy_downloads.load(Ordering::SeqCst),
        4,
        "every title still gets its subtitle from the healthy provider"
    );
}
