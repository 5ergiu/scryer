//! The durable queue of the native HTTP downloader.
//!
//! One JSON document per configured client, under the client's own
//! `download_dir/.scryer-http/`. It holds every job the client knows about,
//! each file's destination, the attempt it is on, and the byte offset that
//! has been durably written. A restart reads it back and resumes from there.
//!
//! The document is replaced atomically (write a sibling, flush, rename over
//! the previous document), so a crash mid-write leaves the last complete
//! document in place. A document that does not parse is reported, never
//! overwritten: the client refuses to start rather than forget its jobs.

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use scryer_application::{AppError, AppResult};
use scryer_plugin_sdk::PluginHttpResource;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;

/// The directory under `download_dir` that holds the client's own state.
pub(crate) const STATE_DIR_NAME: &str = ".scryer-http";

const STATE_FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpJobState {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
}

impl HttpJobState {
    pub(crate) fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum HttpFileRole {
    Media,
    Subtitle,
}

/// The representation identity a partial file was written against. A resume
/// is only trusted when the server still answers for the same one.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RecordedValidator {
    /// A strong entity tag. Weak tags never qualify a resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_modified: Option<String>,
}

impl RecordedValidator {
    /// The value for `If-Range`: the strong entity tag, else the date.
    pub(crate) fn if_range_value(&self) -> Option<&str> {
        self.etag.as_deref().or(self.last_modified.as_deref())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct HttpJobFile {
    pub role: HttpFileRole,
    pub resource: PluginHttpResource,
    /// The finished file. Bytes are written to an attempt-specific `.part`
    /// sibling and renamed here only once complete and verified.
    pub final_path: PathBuf,
    /// 1-based. A resume that cannot be trusted moves to the next attempt,
    /// which writes a fresh `.part` file; earlier partials are left in place.
    pub attempt: u32,
    /// Bytes of the current attempt that were flushed to disk before this
    /// document recorded them.
    pub bytes_done: u64,
    /// The representation length, when the server stated it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator: Option<RecordedValidator>,
    pub complete: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

impl HttpJobFile {
    /// `<final>.part` for the first attempt, `<final>.<n>.part` after it.
    pub(crate) fn part_path(&self) -> PathBuf {
        part_path_for(&self.final_path, self.attempt)
    }
}

pub(crate) fn part_path_for(final_path: &Path, attempt: u32) -> PathBuf {
    let file_name = final_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let part_name = if attempt <= 1 {
        format!("{file_name}.part")
    } else {
        format!("{file_name}.{attempt}.part")
    };
    final_path.with_file_name(part_name)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct HttpJob {
    /// The client item id this client minted for the job.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download_id: Option<String>,
    pub title_id: String,
    pub title_name: String,
    pub facet: String,
    pub release_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub output_basename: String,
    pub output_dir: PathBuf,
    pub files: Vec<HttpJobFile>,
    pub state: HttpJobState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_size_bytes: Option<i64>,
    pub queued_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
}

impl HttpJob {
    pub(crate) fn media_file(&self) -> Option<&HttpJobFile> {
        self.files
            .iter()
            .find(|file| file.role == HttpFileRole::Media)
    }

    /// Bytes written so far, counting finished files at their full length.
    pub(crate) fn durable_bytes(&self) -> u64 {
        self.files
            .iter()
            .map(|file| {
                if file.complete {
                    file.total_bytes.unwrap_or(file.bytes_done)
                } else {
                    file.bytes_done
                }
            })
            .sum()
    }

    /// The whole job's size when every file's length is known.
    pub(crate) fn known_total_bytes(&self) -> Option<u64> {
        self.files.iter().try_fold(0u64, |sum, file| {
            let size = file.total_bytes.or_else(|| {
                file.resource
                    .size_bytes
                    .and_then(|size| u64::try_from(size).ok())
            })?;
            Some(sum.saturating_add(size))
        })
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct HttpQueueDocument {
    pub version: u32,
    pub jobs: Vec<HttpJob>,
}

/// Reads and atomically replaces one client's queue document.
#[derive(Clone, Debug)]
pub(crate) struct HttpStateStore {
    path: PathBuf,
}

impl HttpStateStore {
    pub(crate) fn for_client(download_dir: &Path, client_id: &str) -> Self {
        let safe_id: String = client_id
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                    ch
                } else {
                    '_'
                }
            })
            .take(96)
            .collect();
        let safe_id = if safe_id.is_empty() {
            "default".to_string()
        } else {
            safe_id
        };
        Self {
            path: download_dir
                .join(STATE_DIR_NAME)
                .join(format!("queue-{safe_id}.json")),
        }
    }

    #[cfg(test)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// The stored document, or an empty one when none was ever written.
    pub(crate) async fn load(&self) -> AppResult<HttpQueueDocument> {
        let bytes = match tokio::fs::read(&self.path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(HttpQueueDocument {
                    version: STATE_FORMAT_VERSION,
                    jobs: Vec::new(),
                });
            }
            Err(error) => {
                return Err(AppError::Repository(format!(
                    "the HTTP downloader could not read its queue at {}: {error}",
                    self.path.display()
                )));
            }
        };
        let document: HttpQueueDocument = serde_json::from_slice(&bytes).map_err(|error| {
            AppError::Repository(format!(
                "the HTTP downloader's queue at {} is unreadable ({error}); it was left untouched",
                self.path.display()
            ))
        })?;
        if document.version > STATE_FORMAT_VERSION {
            return Err(AppError::Repository(format!(
                "the HTTP downloader's queue at {} was written by a newer version (format {})",
                self.path.display(),
                document.version
            )));
        }
        Ok(document)
    }

    /// Create the state directory and write the document when none exists,
    /// proving the directory is writable without touching anything else.
    pub(crate) async fn initialize(&self) -> AppResult<()> {
        let parent = self.state_dir()?;
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            AppError::Validation(format!(
                "the download directory is not writable ({}): {error}",
                parent.display()
            ))
        })?;
        match tokio::fs::metadata(&self.path).await {
            Ok(_) => {
                // Opening for write without truncation proves the document
                // can be replaced later and changes nothing.
                tokio::fs::OpenOptions::new()
                    .write(true)
                    .open(&self.path)
                    .await
                    .map(|_| ())
                    .map_err(|error| {
                        AppError::Validation(format!(
                            "the HTTP downloader's queue at {} is not writable: {error}",
                            self.path.display()
                        ))
                    })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.save(&HttpQueueDocument {
                    version: STATE_FORMAT_VERSION,
                    jobs: Vec::new(),
                })
                .await
            }
            Err(error) => Err(AppError::Validation(format!(
                "the HTTP downloader's queue at {} cannot be inspected: {error}",
                self.path.display()
            ))),
        }
    }

    pub(crate) async fn save(&self, document: &HttpQueueDocument) -> AppResult<()> {
        let parent = self.state_dir()?;
        tokio::fs::create_dir_all(parent).await.map_err(|error| {
            AppError::Repository(format!(
                "the HTTP downloader could not create {}: {error}",
                parent.display()
            ))
        })?;
        let mut document = document.clone();
        document.version = STATE_FORMAT_VERSION;
        let bytes = serde_json::to_vec_pretty(&document)
            .map_err(|error| AppError::Repository(error.to_string()))?;
        let staging = self.path.with_extension("json.next");
        let write = async {
            let mut file = tokio::fs::File::create(&staging).await?;
            file.write_all(&bytes).await?;
            file.sync_all().await?;
            drop(file);
            tokio::fs::rename(&staging, &self.path).await
        };
        write.await.map_err(|error| {
            AppError::Repository(format!(
                "the HTTP downloader could not record its queue at {}: {error}",
                self.path.display()
            ))
        })
    }

    fn state_dir(&self) -> AppResult<&Path> {
        self.path.parent().ok_or_else(|| {
            AppError::Repository("the HTTP downloader state path has no parent".into())
        })
    }
}
