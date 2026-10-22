//! Stable names for the ways a plugin call can fail.
//!
//! A failed plugin call travels as [`AppError::PluginFailed`]: the host's
//! English sentence, plus a [`PluginFailure`] whose reason rides beside it as
//! `extensions.reason`, so a client can explain the failure in its own language
//! instead of showing or matching the sentence. A reason names the condition,
//! never the wording, and is API once released: reword a sentence freely, but
//! do not rename or reuse its reason.
//!
//! Most reasons mirror the code a plugin answered with. The rest name what the
//! host saw when the plugin never answered at all.

use scryer_plugin_sdk::{PluginError, PluginErrorCode};

use crate::AppError;
use crate::url_redaction::redact_url_credentials;

/// The longest guest sentence a failure carries to clients, in characters.
const DETAIL_MAX_CHARS: usize = 300;

/// Generic sentences plugin kits put on an error they know nothing about.
/// They add nothing beside a translated reason, so a failure carries no detail
/// for them.
const GENERIC_PUBLIC_MESSAGES: &[&str] = &["indexer component failed"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PluginFailureReason {
    /// The plugin found its configuration unusable.
    InvalidConfig,
    /// The service the plugin talks to rejected its credentials.
    AuthFailed,
    /// The service asked the plugin to slow down.
    RateLimited,
    /// The service could not be reached or did not answer usefully.
    UpstreamUnavailable,
    /// The plugin does not offer what was asked of it.
    Unsupported,
    /// The plugin failed in a way it expects to clear up on its own.
    Temporary,
    /// The plugin failed in a way retrying will not fix.
    Permanent,
    /// The plugin crashed, or the host could not run it.
    HostFailure,
    /// The plugin did not finish within its time budget.
    TimedOut,
    /// The plugin exceeded the memory the host allows it.
    ResourceLimit,
}

impl PluginFailureReason {
    /// Every reason a plugin failure can name. Clients key translations on
    /// these, so adding, renaming, or removing one must be deliberate.
    pub const ALL: &[Self] = &[
        Self::InvalidConfig,
        Self::AuthFailed,
        Self::RateLimited,
        Self::UpstreamUnavailable,
        Self::Unsupported,
        Self::Temporary,
        Self::Permanent,
        Self::HostFailure,
        Self::TimedOut,
        Self::ResourceLimit,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InvalidConfig => "PLUGIN_INVALID_CONFIG",
            Self::AuthFailed => "PLUGIN_AUTH_FAILED",
            Self::RateLimited => "PLUGIN_RATE_LIMITED",
            Self::UpstreamUnavailable => "PLUGIN_UPSTREAM_UNAVAILABLE",
            Self::Unsupported => "PLUGIN_UNSUPPORTED",
            Self::Temporary => "PLUGIN_TEMPORARY_FAILURE",
            Self::Permanent => "PLUGIN_PERMANENT_FAILURE",
            Self::HostFailure => "PLUGIN_HOST_FAILURE",
            Self::TimedOut => "PLUGIN_TIMED_OUT",
            Self::ResourceLimit => "PLUGIN_RESOURCE_LIMIT",
        }
    }

    /// The reason for the code a plugin answered with. Exhaustive, so a code
    /// added to the SDK cannot fall into a generic reason unnoticed.
    pub const fn from_code(code: PluginErrorCode) -> Self {
        match code {
            PluginErrorCode::InvalidConfig => Self::InvalidConfig,
            PluginErrorCode::AuthFailed => Self::AuthFailed,
            PluginErrorCode::RateLimited => Self::RateLimited,
            PluginErrorCode::UpstreamUnavailable => Self::UpstreamUnavailable,
            PluginErrorCode::Unsupported => Self::Unsupported,
            PluginErrorCode::Temporary => Self::Temporary,
            PluginErrorCode::Permanent => Self::Permanent,
        }
    }
}

/// Why a plugin call failed, in terms a client can translate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginFailure {
    pub reason: PluginFailureReason,
    /// The plugin's own public sentence, when it said something specific.
    /// Shown beside the translated reason, never used to choose it.
    pub detail: Option<String>,
}

impl PluginFailure {
    pub fn new(reason: PluginFailureReason) -> Self {
        Self {
            reason,
            detail: None,
        }
    }

    /// The failure a plugin reported. The public sentence becomes the detail,
    /// bounded and with any URL credential removed; the debug sentence never
    /// leaves the host.
    pub fn from_plugin_error(error: &PluginError) -> Self {
        Self {
            reason: PluginFailureReason::from_code(error.code),
            detail: public_detail(&error.public_message),
        }
    }
}

fn public_detail(message: &str) -> Option<String> {
    let message = message.trim();
    if message.is_empty() || GENERIC_PUBLIC_MESSAGES.contains(&message) {
        return None;
    }
    let redacted = redact_url_credentials(message);
    let mut detail = redacted
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(DETAIL_MAX_CHARS + 1)
        .collect::<String>();
    if detail.chars().count() > DETAIL_MAX_CHARS {
        detail = detail.chars().take(DETAIL_MAX_CHARS - 1).collect();
        detail.push('…');
    }
    Some(detail)
}

impl AppError {
    /// A plugin answered `error` to the call `context` names. The sentence
    /// keeps the shape logs have always shown; the plugin's debug sentence is
    /// written to the debug log here and goes no further.
    pub fn plugin_failed(error: &PluginError, context: &str) -> Self {
        if let Some(debug_message) = error
            .debug_message
            .as_deref()
            .map(str::trim)
            .filter(|message| !message.is_empty())
        {
            tracing::debug!(
                context,
                code = ?error.code,
                public_message = %redact_url_credentials(&error.public_message),
                debug_message = %redact_url_credentials(debug_message),
                "plugin reported an error"
            );
        }
        Self::PluginFailed {
            message: format!(
                "{context}: plugin error {:?}: {}",
                error.code, error.public_message
            ),
            failure: PluginFailure::from_plugin_error(error),
        }
    }

    /// A plugin call that failed before the plugin could answer.
    pub fn plugin_host_failed(reason: PluginFailureReason, message: impl Into<String>) -> Self {
        Self::PluginFailed {
            message: message.into(),
            failure: PluginFailure::new(reason),
        }
    }

    /// Why a plugin call failed, if this error is one.
    pub fn plugin_failure(&self) -> Option<&PluginFailure> {
        match self {
            Self::PluginFailed { failure, .. } => Some(failure),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn plugin_error(code: PluginErrorCode, public_message: &str) -> PluginError {
        PluginError {
            code,
            public_message: public_message.to_string(),
            debug_message: Some("upstream said key=example-secret".to_string()),
            retry_after_seconds: None,
            details: None,
        }
    }

    #[test]
    fn every_reason_is_a_distinct_plugin_name_in_screaming_snake() {
        let distinct = PluginFailureReason::ALL
            .iter()
            .map(|reason| reason.as_str())
            .collect::<BTreeSet<_>>();
        assert_eq!(distinct.len(), PluginFailureReason::ALL.len());
        for reason in distinct {
            assert!(reason.starts_with("PLUGIN_"), "{reason}");
            assert!(
                reason
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'),
                "{reason}"
            );
            assert!(!reason.ends_with('_') && !reason.contains("__"), "{reason}");
        }
    }

    #[test]
    fn every_plugin_code_names_its_own_reason() {
        let codes = [
            (PluginErrorCode::InvalidConfig, "PLUGIN_INVALID_CONFIG"),
            (PluginErrorCode::AuthFailed, "PLUGIN_AUTH_FAILED"),
            (PluginErrorCode::RateLimited, "PLUGIN_RATE_LIMITED"),
            (
                PluginErrorCode::UpstreamUnavailable,
                "PLUGIN_UPSTREAM_UNAVAILABLE",
            ),
            (PluginErrorCode::Unsupported, "PLUGIN_UNSUPPORTED"),
            (PluginErrorCode::Temporary, "PLUGIN_TEMPORARY_FAILURE"),
            (PluginErrorCode::Permanent, "PLUGIN_PERMANENT_FAILURE"),
        ];
        let mut seen = BTreeSet::new();
        for (code, expected) in codes {
            let reason = PluginFailureReason::from_code(code);
            assert_eq!(reason.as_str(), expected, "{code:?}");
            assert!(PluginFailureReason::ALL.contains(&reason));
            assert!(seen.insert(reason.as_str()), "{code:?} shares a reason");
        }
    }

    #[test]
    fn a_plugin_error_keeps_its_sentence_and_names_its_reason() {
        let error = AppError::plugin_failed(
            &plugin_error(
                PluginErrorCode::AuthFailed,
                "Tracker login failed: bad password",
            ),
            "indexer indexer_search component",
        );
        assert_eq!(
            error.to_string(),
            "indexer indexer_search component: plugin error AuthFailed: \
             Tracker login failed: bad password"
        );
        let failure = error.plugin_failure().expect("a plugin failure");
        assert_eq!(failure.reason, PluginFailureReason::AuthFailed);
        assert_eq!(
            failure.detail.as_deref(),
            Some("Tracker login failed: bad password")
        );
        assert!(!format!("{error:?}").contains("example-secret"));
    }

    #[test]
    fn a_generic_or_blank_sentence_carries_no_detail() {
        for message in ["indexer component failed", "  ", ""] {
            let failure = PluginFailure::from_plugin_error(&plugin_error(
                PluginErrorCode::Temporary,
                message,
            ));
            assert_eq!(failure.reason, PluginFailureReason::Temporary);
            assert_eq!(failure.detail, None, "{message:?}");
        }
    }

    #[test]
    fn the_detail_is_bounded_single_line_and_free_of_url_credentials() {
        let failure = PluginFailure::from_plugin_error(&plugin_error(
            PluginErrorCode::UpstreamUnavailable,
            "fetch of https://indexer.example/api?apikey=abc123&t=caps failed\nretry later",
        ));
        let detail = failure.detail.expect("a detail");
        assert!(!detail.contains("abc123"), "{detail}");
        assert!(!detail.contains('\n'), "{detail}");

        let long = "x".repeat(DETAIL_MAX_CHARS * 2);
        let failure =
            PluginFailure::from_plugin_error(&plugin_error(PluginErrorCode::Permanent, &long));
        let detail = failure.detail.expect("a detail");
        assert_eq!(detail.chars().count(), DETAIL_MAX_CHARS);
        assert!(detail.ends_with('…'));
    }

    #[test]
    fn only_plugin_failures_name_one() {
        let host = AppError::plugin_host_failed(PluginFailureReason::TimedOut, "timed out");
        assert_eq!(
            host.plugin_failure(),
            Some(&PluginFailure::new(PluginFailureReason::TimedOut))
        );
        assert_eq!(host.to_string(), "timed out");
        assert_eq!(AppError::Repository("boom".into()).plugin_failure(), None);
    }
}
