use super::*;

/// A library root under a temp folder and one title with no recorded folder.
struct MissingFolderFixture {
    app: AppUseCase,
    admin: User,
    title_id: String,
    root: PathBuf,
    tempdir: tempfile::TempDir,
}

async fn seed(facet: MediaFacet) -> MissingFolderFixture {
    let tempdir = tempfile::tempdir().expect("tempdir");
    let root = tempdir.path().join("library");
    std::fs::create_dir_all(&root).expect("create library root");
    let (app, admin, _titles) = bootstrap_with_cutoff_projection_state(
        Arc::new(StoredSettingsRepo::default()),
        Arc::new(StoredQualityProfileRepo::default()),
        Arc::new(MockMediaFileRepo::default()),
    );
    app.update_media_settings(
        &admin,
        facet.clone(),
        empty_update_media_settings_with_roots(vec![build_root_folder_entry(&root, true)]),
    )
    .await
    .expect("save library roots");
    let title_id = add_title(&app, &admin, "Quillmere", facet).await;
    let fixture = MissingFolderFixture {
        app,
        admin,
        title_id,
        root,
        tempdir,
    };
    assert_eq!(fixture.folder_path(&fixture.title_id).await, None);
    fixture
}

async fn add_title(app: &AppUseCase, admin: &User, name: &str, facet: MediaFacet) -> String {
    app.add_title(
        admin,
        NewTitle {
            name: name.into(),
            facet,
            monitored: false,
            ..Default::default()
        },
    )
    .await
    .expect("create title")
    .id
}

impl MissingFolderFixture {
    /// Write a file and record it for `title_id`.
    async fn record_file(&self, title_id: &str, path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).expect("create file folder");
        std::fs::write(path, b"synthetic video").expect("write media file");
        self.app
            .services
            .library
            .media_files
            .insert_media_file(&InsertMediaFileInput {
                title_id: title_id.to_string(),
                file_path: path.to_string_lossy().into_owned(),
                size_bytes: 15,
                role: MediaFileRole::Primary,
                ..Default::default()
            })
            .await
            .expect("record media file");
    }

    async fn folder_path(&self, title_id: &str) -> Option<String> {
        self.app
            .services
            .catalog
            .titles
            .get_by_id(title_id)
            .await
            .expect("load title")
            .expect("title exists")
            .folder_path
            .filter(|path| !path.is_empty())
    }

    async fn title_exists(&self) -> bool {
        self.app
            .services
            .catalog
            .titles
            .get_by_id(&self.title_id)
            .await
            .expect("load title")
            .is_some()
    }

    async fn delete_with_files(&self, fingerprint: String) -> AppResult<()> {
        self.app
            .delete_title(
                &self.admin,
                &self.title_id,
                true,
                Some(DeleteExecutionConfirmation {
                    preview_fingerprint: fingerprint,
                    typed_confirmation: None,
                }),
            )
            .await
    }

    /// The preview and the delete both refuse, and nothing changes.
    async fn assert_refused(&self, kept: &[&Path]) {
        let preview = self
            .app
            .preview_delete_title_files(&self.admin, &self.title_id)
            .await;
        assert!(
            matches!(
                preview,
                Err(AppError::ValidationRefused {
                    reason: "title_folder_unknown",
                    ..
                })
            ),
            "preview should refuse: {preview:?}"
        );
        let bulk = self
            .app
            .preview_delete_titles_files(&self.admin, std::slice::from_ref(&self.title_id))
            .await
            .expect("bulk preview");
        assert!(bulk.items[0].error.is_some(), "bulk preview should refuse");
        let deleted = self.delete_with_files("stale-or-forged".into()).await;
        assert!(
            matches!(
                deleted,
                Err(AppError::ValidationRefused {
                    reason: "title_folder_unknown",
                    ..
                })
            ),
            "delete should refuse: {deleted:?}"
        );
        assert!(self.title_exists().await);
        assert_eq!(self.folder_path(&self.title_id).await, None);
        for path in kept {
            assert_eq!(std::fs::read(path).unwrap(), b"synthetic video");
        }
    }
}

#[tokio::test]
async fn a_title_with_no_folder_and_no_files_deletes_with_nothing_removed_on_disk() {
    let fixture = seed(MediaFacet::Movie).await;
    let bystander = fixture.root.join("Bystander (2026)/Bystander (2026).mkv");
    std::fs::create_dir_all(bystander.parent().unwrap()).unwrap();
    std::fs::write(&bystander, b"synthetic video").unwrap();

    let preview = fixture
        .app
        .preview_delete_title_files(&fixture.admin, &fixture.title_id)
        .await
        .expect("preview a title with nothing on disk");
    assert_eq!(preview.media_count, 0);
    assert_eq!(preview.total_file_count, 0);

    fixture
        .delete_with_files(preview.fingerprint)
        .await
        .expect("delete with files ticked");
    assert!(!fixture.title_exists().await);
    assert_eq!(std::fs::read(&bystander).unwrap(), b"synthetic video");
}

#[tokio::test]
async fn a_title_with_files_and_no_folder_records_the_implied_folder_and_deletes_it() {
    let fixture = seed(MediaFacet::Series).await;
    let title_folder = fixture.root.join("Quillmere");
    fixture
        .record_file(
            &fixture.title_id,
            &title_folder.join("Season 01/Quillmere - S01E01.mkv"),
        )
        .await;
    fixture
        .record_file(
            &fixture.title_id,
            &title_folder.join("Season 02/Quillmere - S02E01.mkv"),
        )
        .await;
    let neighbour = add_title(&fixture.app, &fixture.admin, "Vellicor", MediaFacet::Series).await;
    let neighbour_folder = fixture.root.join("Vellicor");
    fixture
        .app
        .services
        .catalog
        .titles
        .set_folder_path(&neighbour, &neighbour_folder.to_string_lossy())
        .await
        .unwrap();
    let neighbour_file = neighbour_folder.join("Season 01/Vellicor - S01E01.mkv");
    fixture.record_file(&neighbour, &neighbour_file).await;

    let preview = fixture
        .app
        .preview_delete_title_files(&fixture.admin, &fixture.title_id)
        .await
        .expect("preview from the implied folder");
    assert_eq!(preview.media_count, 2);
    assert_eq!(
        fixture.folder_path(&fixture.title_id).await,
        None,
        "a preview records nothing"
    );

    fixture
        .delete_with_files(preview.fingerprint)
        .await
        .expect("delete the implied folder");
    assert!(!fixture.title_exists().await);
    assert!(!title_folder.exists());
    assert_eq!(std::fs::read(&neighbour_file).unwrap(), b"synthetic video");
    assert_eq!(
        fixture.folder_path(&neighbour).await.as_deref(),
        Some(neighbour_folder.to_string_lossy().as_ref())
    );
}

#[tokio::test]
async fn files_split_across_folders_leave_the_title_folder_unknown() {
    let fixture = seed(MediaFacet::Movie).await;
    let first = fixture.root.join("Quillmere (2026)/Quillmere (2026).mkv");
    let second = fixture.root.join("Quillmere Copy/Quillmere (2026).mkv");
    fixture.record_file(&fixture.title_id, &first).await;
    fixture.record_file(&fixture.title_id, &second).await;
    fixture.assert_refused(&[&first, &second]).await;
}

#[tokio::test]
async fn files_directly_in_a_library_root_are_never_claimed() {
    let fixture = seed(MediaFacet::Movie).await;
    let loose = fixture.root.join("Quillmere (2026).mkv");
    let bystander = fixture.root.join("Bystander (2026).mkv");
    std::fs::write(&bystander, b"synthetic video").unwrap();
    fixture.record_file(&fixture.title_id, &loose).await;
    fixture.assert_refused(&[&loose, &bystander]).await;
}

#[tokio::test]
async fn a_folder_another_title_owns_is_never_claimed() {
    let fixture = seed(MediaFacet::Movie).await;
    let folder = fixture.root.join("Shared (2026)");
    let owner = add_title(&fixture.app, &fixture.admin, "Shared", MediaFacet::Movie).await;
    fixture
        .app
        .services
        .catalog
        .titles
        .set_folder_path(&owner, &folder.to_string_lossy())
        .await
        .unwrap();
    let owner_file = folder.join("Shared (2026).mkv");
    fixture.record_file(&owner, &owner_file).await;
    let ours = folder.join("Quillmere (2026).mkv");
    fixture.record_file(&fixture.title_id, &ours).await;
    fixture.assert_refused(&[&owner_file, &ours]).await;
}

#[tokio::test]
async fn a_folder_outside_the_library_is_never_claimed() {
    let fixture = seed(MediaFacet::Movie).await;
    let outside = fixture
        .tempdir
        .path()
        .join("elsewhere/Quillmere (2026)/Quillmere (2026).mkv");
    fixture.record_file(&fixture.title_id, &outside).await;
    fixture.assert_refused(&[&outside]).await;
}
