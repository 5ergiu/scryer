use async_graphql::{Context, ID, MaybeUndefined, Object, Result as GqlResult};
use scryer_application::{
    AppError, AppUseCase, DeleteExecutionConfirmation, DeleteTitlesJobItem, DeleteTitlesJobRequest,
    QueuedReleaseSelection,
};
use scryer_domain::{Library, LibraryPermission, LibraryRoot, MediaFacet, Title, User};

use crate::context::{actor_from_ctx, app_from_ctx, to_gql_error};
use crate::mappers::{
    folder_match_resolution_into_application, from_change_title_folder_result, from_job_run,
    from_library_scan_summary, from_title, monitor_selection_from_input,
};
use crate::types::*;
use crate::utils::{
    ResolvedTitleOptionsInput, map_add_input, merge_title_option_tags, normalize_title_tags,
    parse_download_source_kind,
};

#[derive(Default)]
pub(crate) struct TitleMutations;

fn queued_download_payload(
    title: &scryer_domain::Title,
    job_id: String,
    source_title: Option<String>,
    source_kind: Option<scryer_application::DownloadSourceKind>,
) -> QueueDownloadPayload {
    QueueDownloadPayload {
        status: QueueDownloadResultStatusValue::Queued,
        job_id: Some(job_id.into()),
        title_id: title.id.clone().into(),
        title_name: title.name.clone(),
        source_title,
        source_kind: source_kind.map(DownloadSourceKindValue::from_application),
        conflict: None,
    }
}

fn validation_error(message: impl Into<String>) -> async_graphql::Error {
    to_gql_error(AppError::Validation(message.into()))
}

fn normalized_root_folder_id(root_folder_id: &ID) -> GqlResult<&str> {
    let value = root_folder_id.as_ref().trim();
    if value.is_empty() {
        return Err(validation_error("rootFolderId cannot be empty"));
    }
    Ok(value)
}

fn find_library_root<'a>(
    libraries: &'a [Library],
    root_folder_id: &str,
) -> Option<(&'a Library, &'a LibraryRoot)> {
    libraries.iter().find_map(|library| {
        library
            .roots
            .iter()
            .find(|root| root.id == root_folder_id)
            .map(|root| (library, root))
    })
}

fn root_folder_id_override(root: &LibraryRoot) -> Option<Option<String>> {
    Some(Some(root.id.clone()))
}

fn resolved_title_options(
    facet: &MediaFacet,
    options: TitleOptionsInput,
    root_folder_id: Option<Option<String>>,
) -> GqlResult<ResolvedTitleOptionsInput> {
    let TitleOptionsInput {
        search_languages: normalize_search_option_list(search_languages, true)?,
        search_aliases: normalize_search_option_list(search_aliases, false)?,
