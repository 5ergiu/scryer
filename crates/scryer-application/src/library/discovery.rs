use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Instant, UNIX_EPOCH};

use super::*;
#[cfg(test)]
use crate::library_filename_parser::library_title_walk;
#[cfg(test)]
use crate::library_filename_parser::{LibraryQueryEvidence, parse_library_filename};
pub(crate) use crate::library_filename_parser::{
    LibraryTitleWalk, normalize_folder_name, strip_year_suffix,
};
use crate::stored_paths::{path_to_stored_string, stored_path_to_path_buf};
use scryer_domain::VIDEO_EXTENSIONS;
use unicode_normalization::UnicodeNormalization;

const LIBRARY_SCAN_DISCOVERY_WORK_QUEUE_CAPACITY: usize = 16;
// `_v2` marks the move from SHA-256 to domain-separated BLAKE3. The change
// needs no migration: the freshness check compares the stored scheme to the
// live one, so every `_v1` row reads as changed, is recomputed, and is upserted
// on its next probe. Cost is one extra probe per path on the first pass.
const LIBRARY_PROBE_SIGNATURE_DIRECTORY_SCHEME: &str = "immediate_children_v2";
const LIBRARY_PROBE_SIGNATURE_FILE_SCHEME: &str = "file_snapshot_v2";
pub(crate) const LIBRARY_SCAN_MAX_RECURSIVE_DEPTH: usize = 3;

// Aligned with Sonarr/Radarr special-folder and root-folder exclusion behavior.
const LIBRARY_IGNORED_DIR_NAMES: &[&str] = &[
    "@eadir",
    ".@__thumb",
    "plex versions",
    "$recycle.bin",
    "#recycle",
    "recycler",
    "trash",
    ".trashes",
    "system volume information",
    "lost+found",
    "boot",
    "bootmgr",
    "cache",
    "caches",
    "cachedmessages",
    "msocache",
    "recovery",
    "temporary internet files",
    "windows",
    ".fseventd",
    ".spotlight",
    ".vol",
    ".appledb",
    ".appledesktop",
    ".appledouble",
    ".grab",
];
const LIBRARY_IGNORED_MEDIA_SUBDIR_NAMES: &[&str] = &[
    "extras",
    "extrafanart",
    "backdrops",
    "behind the scenes",
    "deleted scenes",
    "featurette",
    "featurettes",
    "interview",
    "interviews",
    "other",
    "scene",
    "scenes",
    "sample",
    "samples",
    "short",
    "shorts",
    "theme music",
    "trailer",
    "trailers",
];
const LIBRARY_IGNORED_MEDIA_EXTRA_FILE_SUFFIXES: &[&str] = &[
    "trailer",
    "other",
    "behindthescenes",
    "deleted",
    "featurette",
    "interview",
    "scene",
    "short",
];

#[derive(Clone, Debug)]
pub(crate) struct MovieTopLevelEntry {
    pub(crate) path: PathBuf,
    pub(crate) is_dir: bool,
}

#[cfg(test)]
pub(crate) type MovieTopLevelEntryBatchReceiver =
    tokio::sync::mpsc::Receiver<AppResult<Vec<MovieTopLevelEntry>>>;

#[derive(Debug)]
pub(crate) struct TitleFolderBoundary {
    pub(crate) path: PathBuf,
    pub(crate) rejection: Option<String>,
}

pub(crate) struct TitleFolderBoundaryPolicy {
    pub(crate) template_depth: usize,
    pub(crate) stored_folders: Vec<PathBuf>,
    pub(crate) other_roots: Vec<PathBuf>,
    pub(crate) episode_folders: Vec<regex::Regex>,
}

impl TitleFolderBoundaryPolicy {
    fn contains_stored_folder(root: &Path, folder: &Path) -> bool {
        let root = crate::stored_paths::folder_path_identity_key(&path_to_stored_string(root));
        let folder = crate::stored_paths::folder_path_identity_key(&path_to_stored_string(folder));
        match (root, folder) {
            (Some(root), Some(folder)) => root != folder && Path::new(&folder).starts_with(&root),
            _ => false,
        }
    }

    pub(crate) async fn load(
        app: &AppUseCase,
        facet: &MediaFacet,
        library_id: &str,
        root: &Path,
    ) -> AppResult<Self> {
        let default = match facet {
            MediaFacet::Movie => DEFAULT_FOLDER_TEMPLATE_MOVIE,
            MediaFacet::Series => DEFAULT_FOLDER_TEMPLATE_SERIES,
            MediaFacet::Anime => DEFAULT_FOLDER_TEMPLATE_ANIME,
        };
        let template = crate::library::rename::normalize_title_folder_template_or_default(
            app.read_setting_string_value(FOLDER_TEMPLATE_KEY, Some(facet.as_str()))
                .await?,
            default,
            facet.as_str(),
        );
        let titles = app
            .services
            .catalog
            .titles
            .list_delete_preview_info()
            .await?;
        let stored_folders = titles
            .iter()
            .filter(|title| title.library_id == library_id)
            .filter_map(|title| title.folder_path.as_deref())
            .map(stored_path_to_path_buf)
            .filter(|path| Self::contains_stored_folder(root, path))
            .collect();
        let mut other_roots: Vec<_> = app
            .services
            .catalog
            .libraries
            .list(None)
            .await?
            .into_iter()
            .flat_map(|library| library.roots)
            .map(|root| stored_path_to_path_buf(&root.path))
            .filter(|path| Self::contains_stored_folder(root, path))
            .collect();
        other_roots.extend(
            titles
                .into_iter()
                .filter(|title| title.library_id != library_id)
                .filter_map(|title| title.folder_path)
                .map(|path| stored_path_to_path_buf(&path))
                .filter(|path| {
                    Self::contains_stored_folder(root, path)
                        || crate::stored_paths::folder_paths_match(
                            &path_to_stored_string(root),
                            &path_to_stored_string(path),
                        )
                }),
        );
        let episode_folders = episode_folder_patterns(app, facet).await?;
        Ok(Self {
            template_depth: template.split('/').count(),
            stored_folders,
            other_roots,
            episode_folders,
        })
    }
}

pub(crate) async fn episode_folder_patterns(
    app: &AppUseCase,
    facet: &MediaFacet,
) -> AppResult<Vec<regex::Regex>> {
    if *facet == MediaFacet::Movie {
        return Ok(Vec::new());
    }
    let season = crate::library::rename::normalize_season_folder_template_or_default(
        app.read_setting_string_value(SEASON_FOLDER_TEMPLATE_KEY, Some(facet.as_str()))
            .await?,
    );
    let specials = crate::library::rename::normalize_specials_folder_template_or_default(
        app.read_setting_string_value(SPECIALS_FOLDER_TEMPLATE_KEY, Some(facet.as_str()))
            .await?,
    );
    // Naming changes do not rename existing library directories. Keep the
    // conventional episode layouts recognizable alongside current templates.
    [
        season,
        specials,
        "S{season:00}".into(),
        "Season {season}".into(),
        "Specials".into(),
    ]
    .iter()
    .map(|template| crate::library::rename::episode_folder_discovery_pattern(template))
    .collect()
}

/// Enumerate ownership boundaries before invoking the media walker. The
/// traversal retains only a stack of open directories and one bounded batch.
pub(crate) fn stream_title_folder_boundaries(
    root: PathBuf,
    policy: TitleFolderBoundaryPolicy,
    batch_size: usize,
    cancel: tokio_util::sync::CancellationToken,
) -> tokio::sync::mpsc::Receiver<AppResult<Vec<TitleFolderBoundary>>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(LIBRARY_SCAN_DISCOVERY_WORK_QUEUE_CAPACITY);
    tokio::spawn(async move {
        let result = async {
            if batch_size == 0 || policy.template_depth == 0 {
                return Err(AppError::Validation("title boundary depth and batch size must be positive".into()));
            }
            let resolved_root = tokio::fs::canonicalize(&root).await.map_err(|error| AppError::Repository(error.to_string()))?;
            let mut stack = vec![(tokio::fs::read_dir(&root).await.map_err(|error| AppError::Repository(error.to_string()))?, 0usize)];
            let mut batch = Vec::with_capacity(batch_size);
            while let Some((entries, parent_depth)) = stack.last_mut() {
                if cancel.is_cancelled() || sender.is_closed() { return Ok(()); }
                let depth = *parent_depth + 1;
                let Some(entry) = entries.next_entry().await.map_err(|error| AppError::Repository(error.to_string()))? else {
                    stack.pop();
                    continue;
                };
                let path = entry.path();
                let kind = entry.file_type().await.map_err(|error| AppError::Repository(error.to_string()))?;
                if kind.is_symlink() {
                    tokio::select! {
                        _ = cancel.cancelled() => return Ok(()),
                        result = sender.send(Ok(vec![TitleFolderBoundary { path, rejection: Some("linked folder requires explicit ownership resolution before scanning".into()) }])) => if result.is_err() { return Ok(()); },
                    }
                    continue;
                }
                if !kind.is_dir() || should_skip_library_top_level_entry(&path, true) { continue; }
                if policy.other_roots.iter().any(|other| crate::stored_paths::folder_paths_match(
                    &path_to_stored_string(other), &path_to_stored_string(&path),
                )) { continue; }
                let resolved = tokio::fs::canonicalize(&path).await.map_err(|error| AppError::Repository(error.to_string()))?;
                if resolved == resolved_root || !resolved.starts_with(&resolved_root) {
                    tokio::select! {
                        _ = cancel.cancelled() => return Ok(()),
                        result = sender.send(Ok(vec![TitleFolderBoundary { path, rejection: Some("folder resolves outside its library root; resolve the link explicitly before scanning".into()) }])) => if result.is_err() { return Ok(()); },
                    }
                    continue;
                }
                let owned = policy.stored_folders.iter().any(|stored| crate::stored_paths::folder_paths_match(
                    &path_to_stored_string(stored), &path_to_stored_string(&path),
                ));
                let contains_owner = policy.stored_folders.iter().any(|stored| TitleFolderBoundaryPolicy::contains_stored_folder(&path, stored));
                let content = if owned {
                    TitleBoundaryContent { has_media: true, has_child_titles: false }
                } else {
                    // Above title depth these are organizing folders, even if
                    // their children happen to resemble season/specials names.
                    let episode_folders = if depth < policy.template_depth { &[][..] } else { &policy.episode_folders[..] };
                    match inspect_title_boundary_content(&path, episode_folders).await {
                        Ok(content) => content,
                        Err(error) => {
                            tokio::select! {
                                _ = cancel.cancelled() => return Ok(()),
                                result = sender.send(Ok(vec![TitleFolderBoundary { path, rejection: Some(format!("cannot inspect title boundary: {error}")) }])) => if result.is_err() { return Ok(()); },
                            }
                            continue;
                        }
                    }
                };
                let eligible = depth == policy.template_depth && content.has_media && !content.has_child_titles && !contains_owner;
                if owned || content.has_media || depth == policy.template_depth {
                    // A folder at template depth with no media and no child titles is
                    // an empty or junk title folder; it stays a resolvable unmatched
                    // item exactly as the flat scanner recorded it.
                    let rejection = if contains_owner || content.has_child_titles {
                        Some("folder contains another title boundary; resolve the mixed layout explicitly".into())
                    } else if !owned && depth != policy.template_depth && !content.has_media {
                        Some("organizing folder has no eligible title media".into())
                    } else if !owned && depth != policy.template_depth {
                        Some(format!("folder is not at template depth (expected {}, found {depth})", policy.template_depth))
                    } else { None };
                    batch.push(TitleFolderBoundary { path: path.clone(), rejection });
                }
                if !owned && !eligible && (depth <= policy.template_depth || contains_owner) {
                    stack.push((tokio::fs::read_dir(&path).await.map_err(|error| AppError::Repository(error.to_string()))?, depth));
                }
                if batch.len() >= batch_size {
                    let items = std::mem::take(&mut batch);
                    tokio::select! {
                        _ = cancel.cancelled() => return Ok(()),
                        result = sender.send(Ok(items)) => if result.is_err() { return Ok(()); },
                    }
                }
            }
            if !batch.is_empty() {
                tokio::select! {
                    _ = cancel.cancelled() => {},
                    _ = sender.send(Ok(batch)) => {},
                }
            }
            Ok::<(), AppError>(())
        }.await;
        if let Err(error) = result {
            tokio::select! {
                _ = cancel.cancelled() => {},
                _ = sender.send(Err(error)) => {},
            }
        }
    });
    receiver
}

struct TitleBoundaryContent {
    has_media: bool,
    has_child_titles: bool,
}

async fn directory_has_direct_media(path: &Path) -> AppResult<bool> {
    Ok(count_series_loose_root_files(path).await? > 0)
}

async fn directory_has_episode_layout(
    path: &Path,
    episode_folders: &[regex::Regex],
) -> AppResult<bool> {
    if episode_folders.is_empty() {
        return Ok(false);
    }
    let mut entries = tokio::fs::read_dir(path)
        .await
        .map_err(|error| AppError::Repository(error.to_string()))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| AppError::Repository(error.to_string()))?
    {
        if entry
            .file_type()
            .await
            .map_err(|error| AppError::Repository(error.to_string()))?
            .is_dir()
            && episode_folders
                .iter()
                .any(|pattern| pattern.is_match(&entry.file_name().to_string_lossy()))
            && directory_has_direct_media(&entry.path()).await?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

async fn inspect_title_boundary_content(
    path: &Path,
    episode_folders: &[regex::Regex],
) -> AppResult<TitleBoundaryContent> {
    let mut content = TitleBoundaryContent {
        has_media: directory_has_direct_media(path).await?,
        has_child_titles: false,
    };
    let mut entries = tokio::fs::read_dir(path)
        .await
        .map_err(|error| AppError::Repository(error.to_string()))?;
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| AppError::Repository(error.to_string()))?
    {
        let child = entry.path();
        if !entry
            .file_type()
            .await
            .map_err(|error| AppError::Repository(error.to_string()))?
            .is_dir()
            || should_skip_library_top_level_entry(&child, true)
            || child
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| {
                    LIBRARY_IGNORED_MEDIA_SUBDIR_NAMES.contains(&name.to_ascii_lowercase().as_str())
                })
        {
            continue;
        }
        if directory_has_direct_media(&child).await? {
            let season = child
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| episode_folders.iter().any(|pattern| pattern.is_match(name)));
            if season {
                content.has_media = true;
            } else {
                content.has_child_titles = true;
            }
        } else if directory_has_episode_layout(&child, episode_folders).await? {
            content.has_child_titles = true;
        }
    }
    Ok(content)
}

pub(crate) async fn ensure_existing_title_boundary(
    path: &Path,
    episode_folders: &[regex::Regex],
) -> AppResult<()> {
    let content = inspect_title_boundary_content(path, episode_folders).await?;
    if !content.has_media || content.has_child_titles {
        return Err(AppError::Validation("selected folder is an organizing folder or mixed title layout; select the title folder containing its media".into()));
    }
    Ok(())
}

/// An import may reserve a destination that already holds stray files or
/// empty subfolders, but never one that holds other titles' folders.
pub(crate) async fn ensure_folder_holds_no_child_titles(
    path: &Path,
    episode_folders: &[regex::Regex],
) -> AppResult<()> {
    let content = inspect_title_boundary_content(path, episode_folders).await?;
    if content.has_child_titles {
        return Err(AppError::Validation("destination folder already contains other title folders; it is an organizing folder, not a title folder".into()));
    }
    Ok(())
}

#[cfg(test)]
fn extract_library_query_evidence(path: &str, library_root: &str) -> LibraryQueryEvidence {
    parse_library_filename(
        &crate::library_filename_parser::LibraryFilenameParseInput::title_only(
            &stored_path_to_path_buf(path),
            Some(stored_path_to_path_buf(library_root).as_path()),
        ),
    )
    .query_evidence
}

#[cfg(test)]
pub(crate) fn extract_library_queries(
    path: &str,
    library_root: &str,
) -> (Vec<String>, Option<u32>) {
    let evidence = extract_library_query_evidence(path, library_root);
    (evidence.queries, evidence.year)
}

pub(crate) fn elapsed_ms_u64(started_at: Instant) -> u64 {
    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
pub(crate) async fn list_child_directories(root: &Path) -> AppResult<Vec<PathBuf>> {
    Ok(crate::filesystem_walk::FilesystemWalker::new()
        .list_child_directories(root)?
        .into_iter()
        .filter(|path| !should_skip_library_top_level_entry(path, true))
        .collect())
}

pub(crate) async fn count_series_loose_root_files(root: &Path) -> AppResult<usize> {
    let mut entries = tokio::fs::read_dir(root).await.map_err(|error| {
        AppError::Repository(format!("failed to read {}: {error}", root.display()))
    })?;
    let mut count = 0usize;

    while let Some(entry) = entries.next_entry().await.map_err(|error| {
        AppError::Repository(format!("failed to read {}: {error}", root.display()))
    })? {
        let path = entry.path();
        let file_type = entry.file_type().await.map_err(|error| {
            AppError::Repository(format!("failed to inspect {}: {error}", path.display()))
        })?;
        if !file_type.is_file()
            || should_skip_library_top_level_entry(&path, false)
            || path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(is_ignored_media_extra_file_name)
            || !is_allowed_video_path(&path)
        {
            continue;
        }

        count = count.saturating_add(1);
    }

    Ok(count)
}

#[cfg(test)]
pub(crate) async fn list_movie_top_level_entries(
    root: &Path,
) -> AppResult<Vec<MovieTopLevelEntry>> {
    let mut receiver = stream_movie_top_level_entries_batched(root, 64).await?;
    let mut entries = Vec::new();
    while let Some(batch) = receiver.recv().await {
        entries.extend(batch?);
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(entries)
}

#[cfg(test)]
pub(crate) async fn stream_movie_top_level_entries_batched(
    root: &Path,
    batch_size: usize,
) -> AppResult<MovieTopLevelEntryBatchReceiver> {
    let policy = TitleFolderBoundaryPolicy {
        template_depth: 1,
        stored_folders: vec![],
        other_roots: vec![],
        episode_folders: vec![],
    };
    let mut boundaries =
        stream_title_folder_boundaries(root.to_path_buf(), policy, batch_size, Default::default());
    let (sender, receiver) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        while let Some(batch) = boundaries.recv().await {
            let batch = batch.map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| MovieTopLevelEntry {
                        path: entry.path,
                        is_dir: true,
                    })
                    .collect()
            });
            if sender.send(batch).await.is_err() {
                break;
            }
        }
    });
    Ok(receiver)
}

pub(crate) fn is_ignored_library_dir_name(name: &str) -> bool {
    let trimmed = name.trim();
    let normalized = trimmed.to_ascii_lowercase();
    normalized.starts_with('.')
        || normalized.ends_with(".trickplay")
        || is_mangled_short_dir_name(trimmed)
        || LIBRARY_IGNORED_DIR_NAMES.contains(&normalized.as_str())
}

fn is_mangled_short_dir_name(name: &str) -> bool {
    let Some((stem, suffix)) = name.split_once('~') else {
        return false;
    };
    stem.len() == 6
        && !suffix.is_empty()
        && suffix.len() <= 6
        && stem.chars().all(|ch| ch.is_ascii_alphanumeric())
        && suffix.chars().all(|ch| ch.is_ascii_digit())
}

pub(crate) fn is_ignored_library_file_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    normalized == ".ds_store"
        || normalized == "thumbs.db"
        || normalized.starts_with("._")
        || normalized.starts_with(".unmanic")
}

fn normalized_library_name_words(name: &str) -> String {
    name.trim()
        .nfkc()
        .flat_map(char::to_lowercase)
        .map(|ch| if ch.is_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn is_trailer_like_library_dir_name(name: &str) -> bool {
    let normalized = normalized_library_name_words(name);

    normalized
        .split_whitespace()
        .any(|token| token == "trailer" || token == "trailers")
}

pub(crate) fn is_ignored_media_subdir_name(name: &str) -> bool {
    let normalized = name.trim().to_ascii_lowercase();
    let normalized_words = normalized_library_name_words(name);
    LIBRARY_IGNORED_MEDIA_SUBDIR_NAMES.contains(&normalized.as_str())
        || LIBRARY_IGNORED_MEDIA_SUBDIR_NAMES.contains(&normalized_words.as_str())
        || is_trailer_like_library_dir_name(name)
}

pub(crate) fn is_ignored_media_extra_file_name(name: &str) -> bool {
    let stem = Path::new(name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    LIBRARY_IGNORED_MEDIA_EXTRA_FILE_SUFFIXES
        .iter()
        .any(|suffix| stem.ends_with(&format!("-{suffix}")))
}

pub(crate) fn should_skip_library_top_level_entry(path: &Path, is_dir: bool) -> bool {
    let Some(name) = path
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
    else {
        return false;
    };

    if is_dir {
        is_ignored_library_dir_name(name.as_str())
    } else {
        is_ignored_library_file_name(name.as_str())
    }
}

pub(crate) fn should_skip_library_subpath(root: &Path, path: &Path, is_dir: bool) -> bool {
    let Some(relative) = path.strip_prefix(root).ok() else {
        return false;
    };

    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        let Some(name) = name.to_str() else {
            continue;
        };
        if ((components.peek().is_some() || is_dir) && is_ignored_library_dir_name(name))
            || is_ignored_library_file_name(name)
        {
            return true;
        }
    }

    false
}

pub(crate) fn should_skip_movie_library_subpath(root: &Path, path: &Path, is_dir: bool) -> bool {
    should_skip_media_library_subpath(root, path, is_dir)
}

pub(crate) fn should_skip_episodic_library_subpath(root: &Path, path: &Path, is_dir: bool) -> bool {
    should_skip_media_library_subpath(root, path, is_dir)
}

fn should_skip_media_library_subpath(root: &Path, path: &Path, is_dir: bool) -> bool {
    if should_skip_library_subpath(root, path, is_dir) {
        return true;
    }

    let Some(relative) = path.strip_prefix(root).ok() else {
        return false;
    };

    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        let Some(name) = name.to_str() else {
            continue;
        };
        if components.peek().is_some() || is_dir {
            if is_ignored_media_subdir_name(name) {
                return true;
            }
        } else if is_ignored_media_extra_file_name(name) {
            return true;
        }
    }

    false
}

fn is_allowed_video_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .is_some_and(|extension| VIDEO_EXTENSIONS.contains(&extension.as_str()))
}

pub(crate) fn matching_movie_nfo_path(path: &Path) -> Option<String> {
    let same_stem = path.with_extension("nfo");
    if same_stem.is_file() {
        return Some(path_to_stored_string(&same_stem));
    }

    let parent = path.parent()?;
    let movie_nfo = parent.join("movie.nfo");
    if movie_nfo.is_file() {
        return Some(path_to_stored_string(&movie_nfo));
    }

    None
}

pub(crate) async fn matching_movie_nfo_path_async(path: &Path) -> Option<String> {
    let same_stem = path.with_extension("nfo");
    if tokio::fs::metadata(&same_stem)
        .await
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
    {
        return Some(path_to_stored_string(&same_stem));
    }

    let parent = path.parent()?;
    let movie_nfo = parent.join("movie.nfo");
    if tokio::fs::metadata(&movie_nfo)
        .await
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
    {
        return Some(path_to_stored_string(&movie_nfo));
    }

    None
}

pub(crate) fn derive_movie_probe_path(
    root: &Path,
    title: &Title,
    collections: &[Collection],
) -> Option<PathBuf> {
    if let Some(folder_path) = title
        .folder_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(stored_path_to_path_buf)
        .filter(|path| path.starts_with(root))
    {
        return Some(folder_path);
    }

    let mut ordered_paths = collections
        .iter()
        .filter_map(|collection| collection.ordered_path.as_deref())
        .map(stored_path_to_path_buf)
        .filter(|path| path.starts_with(root))
        .collect::<Vec<_>>();
    ordered_paths.sort();
    ordered_paths.dedup();

    let first = ordered_paths.into_iter().next()?;
    if let Some(parent) = first.parent()
        && parent != root
    {
        return Some(parent.to_path_buf());
    }

    Some(first)
}

async fn compute_library_probe_signature(path: &Path) -> AppResult<(String, String)> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || compute_library_probe_signature_blocking(path))
        .await
        .map_err(|error| AppError::Repository(error.to_string()))?
}

#[derive(Clone, Debug)]
struct PendingLibraryProbe {
    path: String,
    scheme: String,
    value: String,
    now: chrono::DateTime<Utc>,
    stored_probe: Option<LibraryProbeSignature>,
}

pub(crate) enum BackgroundRefreshProbeOutcome<T> {
    Unchanged,
    Changed(T),
}

async fn begin_background_refresh_probe(
    app: &AppUseCase,
    title_id: &str,
    path: &Path,
) -> AppResult<Option<PendingLibraryProbe>> {
    let path_string = path_to_stored_string(path);
    let now = Utc::now();
    let (scheme, value) = compute_library_probe_signature(path).await?;
    let stored_probe = app
        .services
        .library
        .library_probe_signatures
        .get_probe_signature(title_id)
        .await?;
    let unchanged = stored_probe.as_ref().is_some_and(|probe| {
        probe.path == path_string
            && probe.probe_signature_scheme.as_deref() == Some(scheme.as_str())
            && probe.probe_signature_value.as_deref() == Some(value.as_str())
    });

    if unchanged {
        app.services
            .library
            .library_probe_signatures
            .upsert_probe_signature(&LibraryProbeSignature {
                title_id: title_id.to_string(),
                path: path_string,
                probe_signature_scheme: Some(scheme),
                probe_signature_value: Some(value),
                last_probed_at: Some(now),
                last_changed_at: stored_probe.and_then(|probe| probe.last_changed_at),
            })
            .await?;
        return Ok(None);
    }

    Ok(Some(PendingLibraryProbe {
        path: path_string,
        scheme,
        value,
        now,
        stored_probe,
    }))
}

async fn persist_background_refresh_probe_result(
    app: &AppUseCase,
    title_id: &str,
    probe: PendingLibraryProbe,
    has_delta: bool,
) -> AppResult<()> {
    app.services
        .library
        .library_probe_signatures
        .upsert_probe_signature(&LibraryProbeSignature {
            title_id: title_id.to_string(),
            path: probe.path,
            probe_signature_scheme: Some(probe.scheme),
            probe_signature_value: Some(probe.value),
            last_probed_at: Some(probe.now),
            last_changed_at: has_delta
                .then_some(probe.now)
                .or_else(|| probe.stored_probe.and_then(|stored| stored.last_changed_at)),
        })
        .await
}

pub(crate) async fn run_background_refresh_probe_with_delta<T, Fut>(
    app: &AppUseCase,
    title_id: &str,
    path: &Path,
    scan_and_diff: Fut,
) -> AppResult<BackgroundRefreshProbeOutcome<T>>
where
    Fut: std::future::Future<Output = AppResult<(T, HashSet<String>, HashSet<String>)>>,
{
    let Some(probe) = begin_background_refresh_probe(app, title_id, path).await? else {
        return Ok(BackgroundRefreshProbeOutcome::Unchanged);
    };

    let (payload, discovered_paths, existing_paths) = scan_and_diff.await?;
    let has_delta = discovered_paths != existing_paths;
    persist_background_refresh_probe_result(app, title_id, probe, has_delta).await?;

    if has_delta {
        Ok(BackgroundRefreshProbeOutcome::Changed(payload))
    } else {
        Ok(BackgroundRefreshProbeOutcome::Unchanged)
    }
}

fn compute_library_probe_signature_blocking(path: PathBuf) -> AppResult<(String, String)> {
    let metadata = std::fs::metadata(&path).map_err(|error| {
        AppError::Repository(format!("failed to inspect {}: {error}", path.display()))
    })?;

    if metadata.is_dir() {
        let mut markers = Vec::new();
        let entries = std::fs::read_dir(&path).map_err(|error| {
            AppError::Repository(format!("failed to read {}: {error}", path.display()))
        })?;
        for entry in entries {
            let entry = entry.map_err(|error| {
                AppError::Repository(format!(
                    "failed to read entry in {}: {error}",
                    path.display()
                ))
            })?;
            let child_path = entry.path();
            let file_type = entry.file_type().map_err(|error| {
                AppError::Repository(format!(
                    "failed to inspect filesystem entry {}: {error}",
                    child_path.display()
                ))
            })?;

            let (kind, child_metadata) = if file_type.is_dir() {
                ("dir", std::fs::metadata(&child_path).ok())
            } else if file_type.is_file() {
                ("file", std::fs::metadata(&child_path).ok())
            } else if file_type.is_symlink() {
                match std::fs::metadata(&child_path) {
                    Ok(metadata) if metadata.is_dir() => ("dir", Some(metadata)),
                    Ok(metadata) if metadata.is_file() => ("file", Some(metadata)),
                    _ => continue,
                }
            } else {
                continue;
            };

            if should_skip_library_top_level_entry(&child_path, kind == "dir") {
                continue;
            }

            let marker = child_metadata
                .as_ref()
                .map(metadata_probe_marker)
                .unwrap_or_else(|| "unknown".to_string());
            let name = child_path
                .file_name()
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default();
            markers.push(format!("{name}|{kind}|{marker}"));
        }
        markers.sort();
        let payload = markers.join("\n");
        Ok((
            LIBRARY_PROBE_SIGNATURE_DIRECTORY_SCHEME.to_string(),
            blake3_identity_hex(HashDomain::LibraryProbe, payload),
        ))
    } else {
        let payload = metadata_probe_marker(&metadata);
        Ok((
            LIBRARY_PROBE_SIGNATURE_FILE_SCHEME.to_string(),
            blake3_identity_hex(HashDomain::LibraryProbe, payload),
        ))
    }
}

fn metadata_probe_marker(metadata: &std::fs::Metadata) -> String {
    let modified = metadata
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
        .map(|value| format!("{}:{}", value.as_secs(), value.subsec_nanos()))
        .unwrap_or_else(|| "unknown".to_string());
    format!("{modified}|{}", metadata.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[cfg(windows)]
    #[test]
    fn stored_folder_selection_uses_windows_path_identity() {
        let root = Path::new(r"C:\Media");
        assert!(TitleFolderBoundaryPolicy::contains_stored_folder(
            root,
            Path::new(r"c:\media\Movie"),
        ));
        for excluded in [r"c:\media", r"C:\Media-other\Movie", r"D:\Media\Movie"] {
            assert!(!TitleFolderBoundaryPolicy::contains_stored_folder(
                root,
                Path::new(excluded)
            ));
        }
    }

    #[cfg(unix)]
    #[test]
    fn stored_folder_selection_preserves_case_sensitive_boundaries() {
        assert!(TitleFolderBoundaryPolicy::contains_stored_folder(
            Path::new("/media/"),
            Path::new("/media/./Movie"),
        ));
        assert!(!TitleFolderBoundaryPolicy::contains_stored_folder(
            Path::new("/media"),
            Path::new("/Media/Movie"),
        ));
        for excluded in ["/media", "/media-other/Movie", "/other/Movie"] {
            assert!(!TitleFolderBoundaryPolicy::contains_stored_folder(
                Path::new("/media"),
                Path::new(excluded)
            ));
        }
    }

    #[test]
    fn episode_discovery_requires_mandatory_structure() {
        use crate::library::rename::episode_folder_discovery_pattern as pattern;
        for template in [
            "{title}",
            "{tvdb_id}",
            "{title} - {tvdb_id}",
            "{?tvdb_id:Specials {tvdb_id}}",
        ] {
            let pattern = pattern(template).unwrap();
            for name in [
                "Show Alpha",
                "12345",
                "Show Alpha - 12345",
                "Specials 12345",
                "",
            ] {
                assert!(!pattern.is_match(name), "{template}: {name}");
            }
        }
        for (template, name) in [
            ("Season {season}", "Season 1"),
            ("{season:02}", "01"),
            ("Extras", "Extras"),
            ("Specials {tvdb_id}", "Specials 12345"),
        ] {
            assert!(pattern(template).unwrap().is_match(name));
        }
    }

    async fn collect_nested_boundaries(
        root: &Path,
        policy: TitleFolderBoundaryPolicy,
    ) -> Vec<TitleFolderBoundary> {
        let mut receiver =
            stream_title_folder_boundaries(root.into(), policy, 1, Default::default());
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            let mut entries = Vec::new();
            while let Some(batch) = receiver.recv().await {
                entries.extend(batch.unwrap());
            }
            entries
        })
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn nested_title_organizing_folders_never_become_episode_folders() {
        use crate::library::rename::episode_folder_discovery_pattern as pattern;
        let root = tempfile::tempdir().unwrap();
        let titles = [
            "A/Alpha",
            "Example Network/Show Alpha",
            "Example Network/Show Beta",
            "GroupA/Release Show",
            "Other Network/Extras",
        ];
        for title in titles {
            let path = root.path().join(title).join("episode.mkv");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"synthetic media").unwrap();
        }
        for specials in ["{title}", "{tvdb_id}", "Extras"] {
            let entries = collect_nested_boundaries(
                root.path(),
                TitleFolderBoundaryPolicy {
                    template_depth: 2,
                    stored_folders: vec![],
                    other_roots: vec![],
                    episode_folders: vec![
                        pattern(specials).unwrap(),
                        pattern("Season {season}").unwrap(),
                    ],
                },
            )
            .await;
            let mut accepted = entries
                .iter()
                .filter(|entry| entry.rejection.is_none())
                .map(|entry| entry.path.clone())
                .collect::<Vec<_>>();
            accepted.sort();
            let mut expected = titles.map(|title| root.path().join(title)).to_vec();
            expected.sort();
            assert_eq!(accepted, expected, "specials template: {specials}");
        }
    }

    #[tokio::test]
    async fn nested_title_ambiguous_specials_require_ownership() {
        use crate::library::rename::episode_folder_discovery_pattern as pattern;
        let root = tempfile::tempdir().unwrap();
        for relative in [
            "Network/Ambiguous/Ambiguous/special.mkv",
            "Network/Conventional/Season 1/episode.mkv",
            "Network/Custom/Specials 12345/special.mkv",
        ] {
            let path = root.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"synthetic media").unwrap();
        }
        for owned in [false, true] {
            let entries = collect_nested_boundaries(
                root.path(),
                TitleFolderBoundaryPolicy {
                    template_depth: 2,
                    stored_folders: if owned {
                        vec![root.path().join("Network/Ambiguous")]
                    } else {
                        vec![]
                    },
                    other_roots: vec![],
                    episode_folders: vec![
                        pattern("{title}").unwrap(),
                        pattern("Season {season}").unwrap(),
                        pattern("Specials {tvdb_id}").unwrap(),
                    ],
                },
            )
            .await;
            for title in ["Conventional", "Custom", "Ambiguous"] {
                let entry = entries
                    .iter()
                    .find(|entry| entry.path == root.path().join("Network").join(title))
                    .unwrap();
                assert_eq!(entry.rejection.is_none(), title != "Ambiguous" || owned);
            }
        }
    }

    #[tokio::test]
    async fn nested_title_owned_layouts_survive_template_changes() {
        let root = tempfile::tempdir().unwrap();
        for relative in ["Series/S01/episode.mkv", "Movie/Disc 1/movie.mkv"] {
            let path = root.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"owned media").unwrap();
        }
        let policy = TitleFolderBoundaryPolicy {
            template_depth: 3,
            stored_folders: vec![root.path().join("Series"), root.path().join("Movie")],
            other_roots: vec![],
            episode_folders: vec![
                crate::library::rename::episode_folder_discovery_pattern("Cycle {season:02}")
                    .unwrap(),
            ],
        };
        let mut receiver =
            stream_title_folder_boundaries(root.path().into(), policy, 1, Default::default());
        let accepted = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            let mut accepted = Vec::new();
            while let Some(batch) = receiver.recv().await {
                for entry in batch.unwrap() {
                    assert!(entry.rejection.is_none());
                    accepted.push(entry.path);
                }
            }
            accepted
        })
        .await
        .unwrap();
        assert_eq!(accepted.len(), 2);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn nested_title_linked_folder_is_reported_without_walking_target() {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("movie.mkv"), b"outside").unwrap();
        std::os::unix::fs::symlink(outside.path(), root.path().join("Linked")).unwrap();
        let policy = TitleFolderBoundaryPolicy {
            template_depth: 1,
            stored_folders: vec![],
            other_roots: vec![],
            episode_folders: vec![],
        };
        let mut receiver =
            stream_title_folder_boundaries(root.path().into(), policy, 1, Default::default());
        let batch = tokio::time::timeout(std::time::Duration::from_secs(30), receiver.recv())
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(batch.len(), 1);
        assert!(
            batch[0]
                .rejection
                .as_deref()
                .unwrap()
                .contains("linked folder")
        );
        assert_eq!(
            std::fs::read(outside.path().join("movie.mkv")).unwrap(),
            b"outside"
        );
    }

    #[tokio::test]
    async fn nested_title_boundaries_keep_siblings_separate_and_stored_depth_authoritative() {
        let root = tempfile::tempdir().unwrap();
        let paths = [
            "E/Emberfall/movie.mkv",
            "E/Echo/movie.mkv",
            "Legacy/movie.mkv",
            "Flat/movie.mkv",
        ];
        let mut before = Vec::new();
        for path in paths {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, b"fixture media").unwrap();
            before.push((path.clone(), blake3::hash(&std::fs::read(path).unwrap())));
        }
        let policy = TitleFolderBoundaryPolicy {
            template_depth: 2,
            stored_folders: vec![root.path().join("Legacy")],
            other_roots: vec![],
            episode_folders: vec![],
        };
        let mut receiver =
            stream_title_folder_boundaries(root.path().into(), policy, 1, Default::default());
        let results = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            let mut results = Vec::new();
            while let Some(batch) = receiver.recv().await {
                results.extend(batch.unwrap());
            }
            results
        })
        .await
        .unwrap();
        let mut accepted = results
            .iter()
            .filter(|entry| entry.rejection.is_none())
            .map(|entry| entry.path.strip_prefix(root.path()).unwrap().to_path_buf())
            .collect::<Vec<_>>();
        accepted.sort();
        assert_eq!(
            accepted,
            vec![
                PathBuf::from("E/Echo"),
                PathBuf::from("E/Emberfall"),
                PathBuf::from("Legacy")
            ]
        );
        assert!(results.iter().any(|entry| {
            entry.path == root.path().join("Flat")
                && entry
                    .rejection
                    .as_deref()
                    .unwrap()
                    .contains("not at template depth")
        }));
        for (path, hash) in before {
            assert_eq!(blake3::hash(&std::fs::read(path).unwrap()), hash);
        }
    }

    #[tokio::test]
    async fn nested_title_boundaries_use_configured_season_layout_and_skip_other_roots() {
        let root = tempfile::tempdir().unwrap();
        for path in [
            "E/Emberfall/Cycle 01/episode.mkv",
            "E/Echo/Specials/episode.mkv",
            "Other/Hidden/movie.mkv",
        ] {
            let path = root.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"media").unwrap();
        }
        let policy = TitleFolderBoundaryPolicy {
            template_depth: 2,
            stored_folders: vec![],
            other_roots: vec![root.path().join("Other")],
            episode_folders: vec![
                crate::library::rename::episode_folder_discovery_pattern("Cycle {season:02}")
                    .unwrap(),
                crate::library::rename::episode_folder_discovery_pattern("Specials").unwrap(),
            ],
        };
        let mut receiver =
            stream_title_folder_boundaries(root.path().into(), policy, 1, Default::default());
        let mut accepted = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while let Some(batch) = receiver.recv().await {
                accepted.extend(
                    batch
                        .unwrap()
                        .into_iter()
                        .filter(|entry| entry.rejection.is_none())
                        .map(|entry| entry.path),
                );
            }
        })
        .await
        .unwrap();
        accepted.sort();
        assert_eq!(
            accepted,
            vec![root.path().join("E/Echo"), root.path().join("E/Emberfall")]
        );
    }

    #[test]
    fn library_title_walk_extracts_simple_title_year_and_ids() {
        let walk = library_title_walk(
            "Correct Movie (2024) [imdb:(tt6263850)] {tmdbid=12345} tvdb://67890 2160p",
        )
        .expect("title walk");

        assert_eq!(walk.title.as_deref(), Some("Correct Movie"));
        assert_eq!(walk.year, Some(2024));
        assert_eq!(walk.imdb_id.as_deref(), Some("tt6263850"));
        assert_eq!(walk.tmdb_id.as_deref(), Some("12345"));
        assert_eq!(walk.tvdb_id.as_deref(), Some("67890"));
    }

    #[test]
    fn library_title_walk_rejects_numeric_only_imdb_values() {
        let walk = library_title_walk("Mislabelled Movie (2024) [imdbid=438631]")
            .expect("title/year should still parse");

        assert_eq!(walk.title.as_deref(), Some("Mislabelled Movie"));
        assert_eq!(walk.year, Some(2024));
        assert_eq!(walk.imdb_id, None);
    }

    #[test]
    fn library_title_walk_preserves_max_inside_title() {
        let walk = library_title_walk("Sand Kettle Fury Road (2015) 2160p").expect("title walk");

        assert_eq!(walk.title.as_deref(), Some("Sand Kettle Fury Road"));
        assert_eq!(walk.year, Some(2015));
    }

    #[test]
    fn library_title_walk_extracts_tvdb_uri_from_series_folder() {
        let walk = library_title_walk("Fathomline (2021) tvdb://366972").expect("title walk");

        assert_eq!(walk.title.as_deref(), Some("Fathomline"));
        assert_eq!(walk.year, Some(2021));
        assert_eq!(walk.tvdb_id.as_deref(), Some("366972"));
    }

    #[test]
    fn ignored_library_dir_name_skips_mangled_short_names() {
        for name in ["D061DC~9", "ABC123~1", "abcdef~12"] {
            assert!(
                is_ignored_library_dir_name(name),
                "{name} should be ignored as a mangled short name"
            );
        }

        for name in ["Canonical Show", "Season 01", "Show~Archive", "ABCDEF"] {
            assert!(
                !is_ignored_library_dir_name(name),
                "{name} should remain a valid library directory"
            );
        }
    }

    #[tokio::test]
    async fn list_child_directories_skips_library_junk_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        tokio::fs::create_dir_all(dir.path().join("Show A"))
            .await
            .expect("show a");
        tokio::fs::create_dir_all(dir.path().join("Canonical Show"))
            .await
            .expect("canonical show dir");
        tokio::fs::create_dir_all(dir.path().join("D061DC~9"))
            .await
            .expect("mangled short name dir");
        tokio::fs::create_dir_all(dir.path().join("@eaDir"))
            .await
            .expect("@eaDir");
        tokio::fs::create_dir_all(dir.path().join(".stfolder"))
            .await
            .expect(".stfolder");
        tokio::fs::create_dir_all(dir.path().join("Show A.trickplay"))
            .await
            .expect(".trickplay");

        let child_dirs = list_child_directories(dir.path())
            .await
            .expect("child dirs");

        assert_eq!(
            child_dirs,
            vec![dir.path().join("Canonical Show"), dir.path().join("Show A"),]
        );
    }

    #[tokio::test]
    async fn list_movie_top_level_entries_skips_junk_entries() {
        let dir = tempfile::tempdir().expect("tempdir");
        tokio::fs::create_dir_all(dir.path().join("Movie A"))
            .await
            .expect("movie dir");
        tokio::fs::create_dir_all(dir.path().join("@eaDir"))
            .await
            .expect("@eaDir");
        tokio::fs::create_dir_all(dir.path().join("Extras"))
            .await
            .expect("extras movie dir");
        tokio::fs::write(dir.path().join("Movie.B.2024.mkv"), b"video")
            .await
            .expect("movie file");
        tokio::fs::write(dir.path().join("Movie.B.2024-trailer.mkv"), b"video")
            .await
            .expect("movie trailer file");
        tokio::fs::write(dir.path().join(".DS_Store"), b"junk")
            .await
            .expect(".DS_Store");

        let entries = list_movie_top_level_entries(dir.path())
            .await
            .expect("movie entries");

        assert_eq!(
            entries
                .iter()
                .map(|entry| entry
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string())
                .collect::<Vec<_>>(),
            vec!["Extras".to_string(), "Movie A".to_string(),]
        );
    }

    #[tokio::test]
    async fn stream_movie_top_level_entries_batched_skips_extra_suffix_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        tokio::fs::create_dir_all(dir.path().join("Movie A"))
            .await
            .expect("movie dir");
        tokio::fs::write(dir.path().join("Movie.B.2024.mkv"), b"video")
            .await
            .expect("movie file");
        tokio::fs::write(dir.path().join("Movie.B.2024-trailer.mkv"), b"video")
            .await
            .expect("movie trailer file");

        let mut receiver = stream_movie_top_level_entries_batched(dir.path(), 1)
            .await
            .expect("movie entry stream");
        let mut entries = Vec::new();
        while let Some(batch) = receiver.recv().await {
            entries.extend(batch.expect("movie entry batch"));
        }

        entries.sort_by(|left, right| left.path.cmp(&right.path));
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string())
                .collect::<Vec<_>>(),
            vec!["Movie A".to_string()]
        );
    }

    #[test]
    fn should_skip_movie_library_subpath_allows_sample_leaf_files() {
        let root = Path::new("/library");
        let path = Path::new("/library/Movie Title/Sample.2024.BluRay.mkv");

        assert!(!should_skip_movie_library_subpath(root, path, false));
    }

    #[test]
    fn should_skip_movie_library_subpath_skips_extra_suffix_leaf_files() {
        let root = Path::new("/library");

        for path in [
            Path::new("/library/Movie Title/Movie.Title-trailer.mkv"),
            Path::new("/library/Movie Title/Movie.Title-other.mkv"),
            Path::new("/library/Movie Title/Movie.Title-behindthescenes.mkv"),
            Path::new("/library/Movie Title/Movie.Title-deleted.mkv"),
            Path::new("/library/Movie Title/Movie.Title-featurette.mkv"),
            Path::new("/library/Movie Title/Movie.Title-interview.mkv"),
            Path::new("/library/Movie Title/Movie.Title-scene.mkv"),
            Path::new("/library/Movie Title/Movie.Title-short.mkv"),
        ] {
            assert!(
                should_skip_movie_library_subpath(root, path, false),
                "expected extra suffix file to be skipped: {}",
                path.display()
            );
        }
    }

    #[test]
    fn should_skip_movie_library_subpath_skips_sonarr_radarr_extra_subdirectories() {
        let root = Path::new("/library");

        for path in [
            Path::new("/library/Movie Title/extras/foo.mkv"),
            Path::new("/library/Movie Title/extrafanart/foo.mkv"),
            Path::new("/library/Movie Title/backdrops/foo.mkv"),
            Path::new("/library/Movie Title/behind the scenes/foo.mkv"),
            Path::new("/library/Movie Title/deleted scenes/foo.mkv"),
            Path::new("/library/Movie Title/featurette/foo.mkv"),
            Path::new("/library/Movie Title/featurettes/foo.mkv"),
            Path::new("/library/Movie Title/interview/foo.mkv"),
            Path::new("/library/Movie Title/interviews/foo.mkv"),
            Path::new("/library/Movie Title/other/foo.mkv"),
            Path::new("/library/Movie Title/scene/foo.mkv"),
            Path::new("/library/Movie Title/scenes/foo.mkv"),
            Path::new("/library/Movie Title/sample/foo.mkv"),
            Path::new("/library/Movie Title/samples/foo.mkv"),
            Path::new("/library/Movie Title/short/foo.mkv"),
            Path::new("/library/Movie Title/shorts/foo.mkv"),
            Path::new("/library/Movie Title/theme.music/foo.mkv"),
            Path::new("/library/Movie Title/theme music/foo.mkv"),
            Path::new("/library/Movie Title/theme-music/foo.mkv"),
            Path::new("/library/Movie Title/theme_music/foo.mkv"),
            Path::new("/library/Movie Title/Trailers/foo.mkv"),
            Path::new("/library/Movie Title/Movie Trailers/foo.mkv"),
            Path::new("/library/Movie Title/12 Tides a Shore (Trailers)/foo.mkv"),
        ] {
            assert!(
                should_skip_movie_library_subpath(root, path, false),
                "expected movie extra folder to be skipped: {}",
                path.display()
            );
        }
    }

    #[test]
    fn should_skip_episodic_library_subpath_skips_sonarr_radarr_extra_subdirectories() {
        let root = Path::new("/library/Anime Show");

        for path in [
            Path::new("/library/Anime Show/extras/foo.mkv"),
            Path::new("/library/Anime Show/Featurettes/foo.mkv"),
            Path::new("/library/Anime Show/Movie Trailers/foo.mkv"),
            Path::new("/library/Anime Show/12 Tides a Shore (Trailers)/foo.mkv"),
            Path::new("/library/Anime Show/theme.music/foo.mkv"),
            Path::new("/library/Anime Show/theme-music/foo.mkv"),
            Path::new("/library/Anime Show/theme_music/foo.mkv"),
        ] {
            assert!(
                should_skip_episodic_library_subpath(root, path, false),
                "expected episodic extra folder to be skipped: {}",
                path.display()
            );
        }
    }

    #[test]
    fn should_skip_media_library_subpath_allows_root_named_extras() {
        let root = Path::new("/library/Extras");

        assert!(!should_skip_movie_library_subpath(
            root,
            Path::new("/library/Extras/Movie.Title.2024.mkv"),
            false,
        ));
        assert!(!should_skip_episodic_library_subpath(
            root,
            Path::new("/library/Extras/Season 1/Episode.S01E01.mkv"),
            false,
        ));
        assert!(should_skip_episodic_library_subpath(
            root,
            Path::new("/library/Extras/Season 1/Extras/Bonus.mkv"),
            false,
        ));
    }

    #[test]
    fn should_skip_episodic_library_subpath_allows_sample_leaf_files() {
        let root = Path::new("/library/Anime Show");
        let path = Path::new("/library/Anime Show/Sample.2024.BluRay.mkv");

        assert!(!should_skip_episodic_library_subpath(root, path, false));
    }

    #[test]
    fn should_skip_episodic_library_subpath_skips_extra_suffix_leaf_files() {
        let root = Path::new("/library/Anime Show");
        let path = Path::new("/library/Anime Show/Episode.S01E01-trailer.mkv");

        assert!(should_skip_episodic_library_subpath(root, path, false));
    }

    #[test]
    fn should_skip_library_subpath_for_trickplay_directories() {
        let root = Path::new("/library");
        let path = Path::new("/library/Show Name/Show.Name.S01E01.trickplay");

        assert!(should_skip_library_subpath(root, path, true));
    }

    #[test]
    fn should_skip_arr_special_and_recycle_directories() {
        for path in [
            Path::new("/library/$RECYCLE.BIN"),
            Path::new("/library/#recycle"),
            Path::new("/library/recycler"),
            Path::new("/library/trash"),
            Path::new("/library/.Trashes"),
            Path::new("/library/System Volume Information"),
            Path::new("/library/lost+found"),
            Path::new("/library/Windows"),
            Path::new("/library/Cache"),
            Path::new("/library/.grab"),
            Path::new("/library/.AppleDouble"),
        ] {
            assert!(
                should_skip_library_top_level_entry(path, true),
                "expected special folder to be skipped: {}",
                path.display()
            );
        }

        let root = Path::new("/library");
        for path in [
            Path::new("/library/Movie Title/$RECYCLE.BIN/foo.mkv"),
            Path::new("/library/Movie Title/#recycle/foo.mkv"),
            Path::new("/library/Movie Title/lost+found/foo.mkv"),
            Path::new("/library/Movie Title/trash/foo.mkv"),
        ] {
            assert!(
                should_skip_movie_library_subpath(root, path, false),
                "expected nested special folder to be skipped: {}",
                path.display()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn should_not_skip_non_utf8_top_level_entries() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let path = Path::new(OsStr::from_bytes(b"/library/\xFFMovie.mkv"));
        assert!(!should_skip_library_top_level_entry(path, false));
    }

    #[test]
    fn derive_movie_probe_path_ignores_stale_folder_path_from_other_root() {
        let root = Path::new("/Volumes/Media/Movies");
        let title = Title {
            id: "title-1".into(),
            name: "Existing Movie".into(),
            facet: MediaFacet::Movie,
            library_id: scryer_domain::default_library_id_for_facet(&MediaFacet::Movie),
            root_folder_id: scryer_domain::root_folder_id_for_path("/data/test"),
            monitored: true,
            tags: vec![],
            canonical_tags: vec![],
            external_ids: vec![],
            created_by: None,
            created_at: Utc::now(),
            year: Some(2024),
            overview: None,
            poster_url: None,
            poster_source_url: None,
            background_url: None,
            background_source_url: None,
            sort_title: None,
            catalog_sort_key: String::new(),
            slug: None,
            imdb_id: None,
            runtime_minutes: None,
            popularity: None,
            content_status: None,
            language: None,
            first_aired: None,
            network: None,
            studio: None,
            country: None,
            aliases: vec![],
            tagged_aliases: vec![],
            metadata_language: None,
            metadata_fetched_at: None,
            min_availability: None,
            digital_release_date: None,
            folder_path: Some("/Volumes/Archive/Movies/Existing Movie".into()),
        };
        let collections = vec![Collection {
            id: "collection-1".into(),
            title_id: title.id.clone(),
            collection_type: CollectionType::Movie,
            collection_index: "1".into(),
            label: None,
            ordered_path: Some(
                "/Volumes/Media/Movies/Existing Movie/Existing.Movie.2024.2160p.WEB-DL.mkv".into(),
            ),
            narrative_order: None,
            first_episode_number: None,
            last_episode_number: None,
            monitored: true,
            created_at: Utc::now(),
        }];

        assert_eq!(
            derive_movie_probe_path(root, &title, &collections),
            Some(PathBuf::from("/Volumes/Media/Movies/Existing Movie"))
        );
    }
}
