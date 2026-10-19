//! What the native HTTP downloader fetches from a release, and where it
//! writes it.
//!
//! The host's selection is a candidate set: every resource of a transport
//! the client declared, minus thumbnails. This picks the one media file that
//! best fits the profile's ceiling and preferences, plus one subtitle sidecar
//! per wanted language, and names each output after the host's basename.

use std::path::{Path, PathBuf};

use scryer_application::{AppError, AppResult};
use scryer_plugin_sdk::{
    HttpResourceRole, HttpSubtitleMode, HttpSubtitleRole, HttpTransport, PluginHttpDownloadSource,
    PluginHttpResource,
};

use super::state::HttpFileRole;

/// Longest output basename accepted from the host. The host keeps its own
/// well under this; anything longer is not a name this client wrote.
const MAX_BASENAME_LEN: usize = 200;
const MAX_EXTENSION_LEN: usize = 8;

/// One file the job will write.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlannedFile {
    pub role: HttpFileRole,
    pub resource: PluginHttpResource,
    pub final_path: PathBuf,
    pub language: Option<String>,
}

/// Decide what to fetch and where it goes, or refuse with the reason.
///
/// Refusals carry a stable leading token so callers and logs can tell them
/// apart: `unsupported_transport` when the release offers nothing this client
/// can fetch, `separate_tracks` when only an audio and video pair is on
/// offer, `no_media_resource` when nothing playable was selected.
pub(crate) fn plan_download(
    source: &PluginHttpDownloadSource,
    download_dir: &Path,
) -> AppResult<(PathBuf, Vec<PlannedFile>)> {
    let basename = validate_output_basename(&source.policy.output_basename)?;
    let offered = source.release.offered_transports();
    if !offered.contains(&HttpTransport::DirectFile) {
        return Err(AppError::Validation(format!(
            "unsupported_transport: this release offers only {}; the HTTP downloader fetches direct files",
            offered
                .iter()
                .map(transport_label)
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    let selected: Vec<&PluginHttpResource> = if source.selected.resources.is_empty() {
        source.release.resources.iter().collect()
    } else {
        source
            .selected
            .resources
            .iter()
            .filter_map(|index| {
                usize::try_from(*index)
                    .ok()
                    .and_then(|index| source.release.resources.get(index))
            })
            .collect()
    };
    let direct: Vec<&PluginHttpResource> = selected
        .into_iter()
        .filter(|resource| resource.transport == HttpTransport::DirectFile)
        .collect();

    let media_candidates: Vec<&PluginHttpResource> = direct
        .iter()
        .copied()
        .filter(|resource| is_complete_media(resource))
        .collect();
    if media_candidates.is_empty() {
        let needs_muxing = direct.iter().any(|resource| {
            matches!(
                resource.role,
                HttpResourceRole::Video | HttpResourceRole::Audio
            )
        });
        if needs_muxing {
            return Err(AppError::Validation(
                "separate_tracks: this release offers separate audio and video tracks, which need muxing; the HTTP downloader does not mux"
                    .to_string(),
            ));
        }
        return Err(AppError::Validation(
            "no_media_resource: the selection holds no direct media file to download".to_string(),
        ));
    }
    let media = choose_media(&media_candidates, source);

    let output_dir = download_dir.join(basename);
    let media_extension = extension_for(media, None).unwrap_or_else(|| "bin".to_string());
    let mut files = vec![PlannedFile {
        role: HttpFileRole::Media,
        resource: media.clone(),
        final_path: output_dir.join(format!("{basename}.{media_extension}")),
        language: None,
    }];

    let subtitles = choose_subtitles(&direct, source)?;
    for subtitle in subtitles {
        let language = subtitle
            .subtitle
            .as_ref()
            .and_then(|track| track.language.as_deref())
            .and_then(sanitize_token);
        let extension = extension_for(
            subtitle,
            subtitle
                .subtitle
                .as_ref()
                .and_then(|track| track.format.as_deref()),
        )
        .unwrap_or_else(|| "srt".to_string());
        let mut stem = basename.to_string();
        if let Some(language) = language.as_deref() {
            stem.push('.');
            stem.push_str(language);
        }
        match subtitle.subtitle.as_ref().and_then(|track| track.role) {
            Some(HttpSubtitleRole::Forced) => stem.push_str(".forced"),
            Some(HttpSubtitleRole::Sdh) => stem.push_str(".sdh"),
            _ => {}
        }
        let mut final_path = output_dir.join(format!("{stem}.{extension}"));
        let mut suffix = 2;
        while files.iter().any(|file| file.final_path == final_path) {
            final_path = output_dir.join(format!("{stem}.{suffix}.{extension}"));
            suffix += 1;
        }
        files.push(PlannedFile {
            role: HttpFileRole::Subtitle,
            resource: subtitle.clone(),
            final_path,
            language,
        });
    }

    Ok((output_dir, files))
}

/// The host's basename, refused unless it is a single plain path component.
pub(crate) fn validate_output_basename(raw: &str) -> AppResult<&str> {
    let basename = raw.trim();
    let valid = !basename.is_empty()
        && basename.len() <= MAX_BASENAME_LEN
        && basename != "."
        && basename != ".."
        && !basename.starts_with('.')
        && basename
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'));
    if valid {
        Ok(basename)
    } else {
        Err(AppError::Validation(format!(
            "the output name {raw:?} is not a plain file name"
        )))
    }
}

/// A file that plays on its own: a muxed file, or a video resource that does
/// not depend on a separate audio track.
fn is_complete_media(resource: &PluginHttpResource) -> bool {
    match resource.role {
        HttpResourceRole::Muxed => true,
        HttpResourceRole::Video => !resource.separate_tracks,
        _ => false,
    }
}

/// The tallest candidate within the profile's ceiling (the shortest one when
/// none fits), then the preferred codec, then the preferred container, then
/// the indexer's order.
fn choose_media<'a>(
    candidates: &[&'a PluginHttpResource],
    source: &PluginHttpDownloadSource,
) -> &'a PluginHttpResource {
    let height = |resource: &PluginHttpResource| resource.video.as_ref().and_then(|v| v.height);
    let ceiling = source.selected.max_height;
    let within: Vec<&'a PluginHttpResource> = candidates
        .iter()
        .copied()
        .filter(|resource| match (ceiling, height(resource)) {
            (Some(ceiling), Some(height)) => height <= ceiling,
            _ => true,
        })
        .collect();
    let preference_rank = |preferences: &[String], value: Option<&str>| -> usize {
        value
            .and_then(|value| {
                preferences
                    .iter()
                    .position(|preferred| preferred.eq_ignore_ascii_case(value))
            })
            .unwrap_or(preferences.len())
    };
    let rank = |resource: &PluginHttpResource| {
        (
            preference_rank(
                &source.selected.video_codec_preference,
                resource.video.as_ref().and_then(|v| v.codec.as_deref()),
            ),
            preference_rank(
                &source.selected.container_preference,
                resource.container.as_deref(),
            ),
        )
    };
    if within.is_empty() {
        return candidates
            .iter()
            .copied()
            .min_by_key(|resource| (height(resource).unwrap_or(u32::MAX), rank(resource)))
            .expect("candidates is not empty");
    }
    // `min_by_key` keeps the first of equal keys, so the indexer's order
    // breaks ties.
    within
        .iter()
        .copied()
        .min_by_key(|resource| {
            (
                std::cmp::Reverse(height(resource).unwrap_or(0)),
                rank(resource),
            )
        })
        .expect("within is not empty")
}

/// One sidecar per wanted language, in the order the languages were asked
/// for, when the selection asks for sidecars at all.
fn choose_subtitles<'a>(
    direct: &[&'a PluginHttpResource],
    source: &PluginHttpDownloadSource,
) -> AppResult<Vec<&'a PluginHttpResource>> {
    let wants_sidecars = matches!(
        source.selected.subtitle_mode,
        HttpSubtitleMode::SidecarPreferred | HttpSubtitleMode::SidecarRequired
    );
    if !wants_sidecars {
        return Ok(Vec::new());
    }
    let available: Vec<&'a PluginHttpResource> = direct
        .iter()
        .copied()
        .filter(|resource| resource.role == HttpResourceRole::Subtitle)
        .collect();
    let mut chosen: Vec<&'a PluginHttpResource> = Vec::new();
    for wanted in &source.selected.subtitle_languages {
        let found = available.iter().copied().find(|resource| {
            resource
                .subtitle
                .as_ref()
                .and_then(|track| track.language.as_deref())
                .is_some_and(|language| language_matches(language, wanted))
                && !chosen.iter().any(|picked| std::ptr::eq(*picked, *resource))
        });
        if let Some(found) = found {
            chosen.push(found);
        }
    }
    if chosen.is_empty()
        && source.selected.subtitle_mode == HttpSubtitleMode::SidecarRequired
        && !source.selected.subtitle_languages.is_empty()
    {
        return Err(AppError::Validation(
            "no_subtitle_resource: subtitle sidecars are required but none in a wanted language was offered"
                .to_string(),
        ));
    }
    Ok(chosen)
}

/// `en` matches `en`, `en-US` and `en_GB`; comparison ignores case.
fn language_matches(offered: &str, wanted: &str) -> bool {
    let primary = |value: &str| {
        value
            .trim()
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    let offered = offered.trim();
    offered.eq_ignore_ascii_case(wanted.trim()) || primary(offered) == primary(wanted)
}

/// The output extension: the stated format or container, else the URL's
/// own extension. Only short alphanumeric tokens are accepted, so nothing
/// from the indexer can reach the file system as a path.
pub(crate) fn extension_for(resource: &PluginHttpResource, format: Option<&str>) -> Option<String> {
    format
        .and_then(sanitize_extension)
        .or_else(|| resource.container.as_deref().and_then(sanitize_extension))
        .or_else(|| url_extension(&resource.url))
}

fn url_extension(raw: &str) -> Option<String> {
    let url = url::Url::parse(raw).ok()?;
    let last = url.path_segments()?.next_back()?.to_string();
    let (_, extension) = last.rsplit_once('.')?;
    sanitize_extension(extension)
}

fn sanitize_extension(raw: &str) -> Option<String> {
    let value = raw.trim().trim_start_matches('.').to_ascii_lowercase();
    (!value.is_empty()
        && value.len() <= MAX_EXTENSION_LEN
        && value.chars().all(|ch| ch.is_ascii_alphanumeric()))
    .then_some(value)
}

fn sanitize_token(raw: &str) -> Option<String> {
    let value: String = raw
        .trim()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .take(16)
        .collect();
    (!value.is_empty()).then(|| value.to_ascii_lowercase())
}

fn transport_label(transport: &HttpTransport) -> &'static str {
    match transport {
        HttpTransport::ProviderPage => "provider_page",
        HttpTransport::DirectFile => "direct_file",
        HttpTransport::Hls => "hls",
        HttpTransport::Dash => "dash",
    }
}
