//! A custom recycle bin must stay outside every library root, whichever root
//! the file being recycled comes from. A recycle that is refused leaves the
//! file exactly where it was.

use super::*;

/// A library store whose every read fails, standing in for a catalog that
/// cannot be read at the moment a file is recycled.
struct UnreadableLibraryRepo;

fn unreadable() -> AppError {
    AppError::Repository("library store unavailable".into())
}

#[async_trait]
impl LibraryRepository for UnreadableLibraryRepo {
    async fn list(&self, _facet: Option<MediaFacet>) -> AppResult<Vec<Library>> {
        Err(unreadable())
    }
    async fn get_by_id(&self, _id: &str) -> AppResult<Option<Library>> {
        Err(unreadable())
    }
    async fn default_for_facet(&self, _facet: MediaFacet) -> AppResult<Option<Library>> {
        Err(unreadable())
    }
    async fn create(&self, _library: Library, _roots: Vec<LibraryRootDraft>) -> AppResult<Library> {
        Err(unreadable())
    }
    async fn update(
        &self,
        _library_id: &str,
        _name: String,
        _slug: String,
        _roots: Vec<LibraryRootDraft>,
    ) -> AppResult<Library> {
        Err(unreadable())
    }
    async fn set_root_path(&self, _root_id: &str, _path: &str) -> AppResult<Library> {
        Err(unreadable())
    }
    async fn delete_library(&self, _library_id: &str) -> AppResult<bool> {
        Err(unreadable())
    }
    async fn app_permission_mask_for_user(&self, _user_id: &str) -> AppResult<AppPermissionMask> {
        Err(unreadable())
    }
    async fn set_app_permission_mask_for_user(
        &self,
        _user_id: &str,
        _permissions: AppPermissionMask,
    ) -> AppResult<()> {
        Err(unreadable())
    }
    async fn permission_masks_for_user(&self, _user_id: &str) -> AppResult<Vec<LibraryGrant>> {
        Err(unreadable())
    }
    async fn set_grants_for_user(
        &self,
        _user_id: &str,
        _grants: Vec<LibraryGrant>,
    ) -> AppResult<()> {
        Err(unreadable())
    }
    async fn title_library_id(&self, _title_id: &str) -> AppResult<Option<String>> {
        Err(unreadable())
    }
}

struct Fixture {
    temp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("fixture tempdir");
        for dir in ["library-a", "library-b"] {
            std::fs::create_dir_all(temp.path().join(dir)).expect("create library root");
        }
        Self { temp }
    }

    fn root_a(&self) -> PathBuf {
        self.temp.path().join("library-a")
    }

    fn root_b(&self) -> PathBuf {
        self.temp.path().join("library-b")
    }

    /// A media file plus an unrelated neighbour in the same title folder.
    fn seed_media(&self, root: &Path, name: &str) -> (PathBuf, PathBuf) {
        let folder = root.join(format!("{name} (2001)"));
        std::fs::create_dir_all(&folder).expect("create title folder");
        let media = folder.join(format!("{name}.mkv"));
        let neighbour = folder.join(format!("{name}.nfo"));
        std::fs::write(&media, format!("{name} media")).expect("write media file");
        std::fs::write(&neighbour, format!("{name} neighbour")).expect("write neighbour");
        (media, neighbour)
    }
}

fn settings_with_bin(bin: Option<&Path>) -> Arc<StoredSettingsRepo> {
    let settings = StoredSettingsRepo::default();
    if let Some(bin) = bin {
        let value = serde_json::to_string(&bin.to_string_lossy()).expect("encode bin path");
        settings
            .values
            .try_lock()
            .expect("fresh settings store")
            .insert(
                (
                    SETTINGS_SCOPE_MEDIA.to_string(),
                    RECYCLE_BIN_PATH_KEY.to_string(),
                    None,
                ),
                value,
            );
    }
    Arc::new(settings)
}

fn removal_manifest(source: &Path) -> crate::recycle_bin::RecycleManifest {
    crate::recycle_bin::RecycleManifest {
        schema: None,
        entry_id: None,
        source_operation_id: None,
        recycled_at: Utc::now().to_rfc3339(),
        original_path: source.to_string_lossy().to_string(),
        original_file_id: None,
        size_bytes: 0,
        title_id: None,
        media_root: None,
        reason: "file_deleted".to_string(),
        status: None,
        replacement_file_id: None,
        replacement_path: None,
        media_row: None,
    }
}

fn assert_untouched(path: &Path, content: &str) {
    assert_eq!(
        std::fs::read_to_string(path).expect("file is still in place"),
        content
    );
}

async fn movie_app(root: &Path) -> (AppUseCase, User) {
    let (app, user, _) =
        bootstrap_movie_scan_app(root, Vec::new(), Arc::new(EmptySearchMetadataGateway)).await;
    (app, user)
}

#[tokio::test]
async fn unreadable_library_roots_refuse_a_custom_bin_and_keep_the_file() {
    let fixture = Fixture::new();
    let bin = fixture.temp.path().join("outside-bin");
    let (app, _) = movie_app(&fixture.root_a()).await;
    let app = app.with_test_overrides(|services| {
        services
            .with_settings(settings_with_bin(Some(&bin)))
            .with_libraries(Arc::new(UnreadableLibraryRepo))
    });
    let root_a = fixture.root_a();
    let (media, neighbour) = fixture.seed_media(&root_a, "Synthetic Feature");

    let by_str = app
        .recycle_bin_config_for_media_root(Some(root_a.to_string_lossy().as_ref()))
        .await;
    let by_path = app
        .recycle_bin_config_for_media_root_path(Some(&root_a))
        .await;
    let for_recycling = app
        .recycle_bin_configs_for_recycling(vec![root_a.to_string_lossy().to_string()])
        .await;
    assert_eq!(for_recycling.len(), 1);
    for config in [&by_str, &by_path, &for_recycling[0].1] {
        let error = config
            .validation_error
            .as_deref()
            .expect("a bin that cannot be checked is refused");
        assert!(
            error.contains("could not be checked against the library roots"),
            "unexpected refusal: {error}"
        );
        assert!(!config.cleanup_enabled);
        assert_eq!(config.source_roots, vec![root_a.clone()]);

        crate::recycle_bin::recycle_file(config, &media, removal_manifest(&media))
            .await
            .expect_err("the recycle is refused");
        assert_untouched(&media, "Synthetic Feature media");
        assert_untouched(&neighbour, "Synthetic Feature neighbour");
    }
    assert!(!bin.exists(), "a refused recycle creates no bin");
}

#[tokio::test]
async fn unreadable_library_roots_leave_the_default_bin_working() {
    let fixture = Fixture::new();
    let (app, _) = movie_app(&fixture.root_a()).await;
    let app = app.with_test_overrides(|services| {
        services
            .with_settings(settings_with_bin(None))
            .with_libraries(Arc::new(UnreadableLibraryRepo))
    });
    let root_a = fixture.root_a();
    let (media, neighbour) = fixture.seed_media(&root_a, "Synthetic Feature");

    let config = app
        .recycle_bin_config_for_media_root(Some(root_a.to_string_lossy().as_ref()))
        .await;
    assert_eq!(config.validation_error, None);
    assert_eq!(config.base_path, root_a.join(".scryer-recycle"));

    let result = crate::recycle_bin::recycle_file(&config, &media, removal_manifest(&media))
        .await
        .expect("the default bin needs no library roots")
        .expect("file recycled");
    assert!(!media.exists());
    assert_untouched(&result.recycled_path, "Synthetic Feature media");
    assert_untouched(&neighbour, "Synthetic Feature neighbour");
}

#[tokio::test]
async fn upgrade_refuses_a_custom_bin_inside_another_library_root_and_keeps_the_old_file() {
    let fixture = Fixture::new();
    let root_a = fixture.root_a();
    let root_b = fixture.root_b();
    let (app, user) = movie_app(&root_a).await;
    let now = Utc::now();
    app.services
        .catalog
        .libraries
        .create(
            Library {
                id: "synthetic-library-b".to_string(),
                facet: MediaFacet::Movie,
                name: "Synthetic Library B".to_string(),
                slug: "synthetic-library-b".to_string(),
                is_default: false,
                roots: Vec::new(),
                created_at: now,
                updated_at: now,
            },
            vec![LibraryRootDraft {
                path: root_b.to_string_lossy().to_string(),
                is_default: true,
            }],
        )
        .await
        .expect("create second library");
    let (media, neighbour) = fixture.seed_media(&root_a, "Synthetic Feature");
    let title = create_movie_title_with_folder(
        &app,
        &user,
        "Synthetic Feature",
        media.parent().expect("title folder"),
    )
    .await;
    let bin = root_b.join("shared-bin");
    let app =
        app.with_test_overrides(|services| services.with_settings(settings_with_bin(Some(&bin))));
    let old_file = TitleMediaFile {
        id: "synthetic-old-file".to_string(),
        title_id: title.id.clone(),
        file_path: media.to_string_lossy().to_string(),
        ..Default::default()
    };

    let context = crate::upgrade::resolve_old_file_recycle_context(&app, &title, &old_file)
        .await
        .expect("the old file's root resolves");
    let error = context
        .recycle_config
        .validation_error
        .as_deref()
        .expect("a bin inside another library's root is refused");
    assert!(
        error.contains(&*root_b.to_string_lossy()),
        "the refusal names the root the bin sits in: {error}"
    );
    assert_eq!(context.recycle_config.source_roots, vec![root_a.clone()]);

    let refused = crate::recycle_bin::recycle_replaced_media_file(
        &context.recycle_config,
        &media,
        crate::recycle_bin::ReplacedMediaRecycleMetadata {
            original_path: &old_file.file_path,
            original_file_id: &old_file.id,
            size_bytes: 0,
            title_id: &title.id,
            media_root: Some(context.media_root.as_str()),
            media_row: None,
        },
        true,
    )
    .await;
    assert!(refused.is_err(), "the recycle is refused");
    assert_untouched(&media, "Synthetic Feature media");
    assert_untouched(&neighbour, "Synthetic Feature neighbour");
    assert!(!bin.exists(), "a refused recycle creates no bin");
}
