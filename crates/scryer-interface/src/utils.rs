use async_graphql::Result as GqlResult;
use scryer_domain::{ExternalId, NewTitle};

use crate::context::to_gql_error;
use crate::types::{AddTitleInput, DownloadSourceKindValue, IntoApplication};

pub(crate) struct ResolvedTitleOptionsInput {
    pub search_languages: Option<Option<Vec<String>>>,
    pub search_aliases: Option<Option<Vec<String>>>,
    /// `None` preserves the stored value; `Some(None)` clears it; `Some(Some(_))` sets it.
    pub min_availability: Option<Option<String>>,
    pub quality_profile_id: Option<Option<String>>,
    pub root_folder_id: Option<Option<String>>,
    pub monitor_type: Option<Option<String>>,
    pub use_season_folders: Option<Option<bool>>,
    pub metadata_language: Option<Option<String>>,
    pub monitor_specials: Option<Option<bool>>,
    pub inter_season_movies: Option<Option<bool>>,
    pub filler_policy: Option<Option<String>>,
    pub recap_policy: Option<Option<String>>,
    pub release_numbering: Option<Option<String>>,
    pub monitor_selection: Option<Option<scryer_domain::MonitorSelection>>,
}

impl ResolvedTitleOptionsInput {
    pub(crate) fn to_application_patch(&self) -> scryer_application::TitleOptionsPatch {
        scryer_application::TitleOptionsPatch {
            search_languages: self.search_languages.clone(),
            search_aliases: self.search_aliases.clone(),
