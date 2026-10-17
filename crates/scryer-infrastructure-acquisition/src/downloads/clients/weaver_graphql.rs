pub const TEST_CONNECTION_QUERY: &str = include_str!("weaver/test_connection.graphql");
pub const VERSION_COMPAT_QUERY: &str = include_str!("weaver/version_compat.graphql");
pub const QUEUE_ITEMS_QUERY: &str = include_str!("weaver/queue_items.graphql");
pub const HISTORY_ITEM_QUERY: &str = include_str!("weaver/history_item.graphql");
pub const HISTORY_ITEMS_QUERY: &str = include_str!("weaver/history_items.graphql");
// The history reads for a Weaver that does not report `postedFileNames`.
pub const HISTORY_ITEM_COMPAT_QUERY: &str = include_str!("weaver/history_item_compat.graphql");
pub const HISTORY_ITEMS_COMPAT_QUERY: &str = include_str!("weaver/history_items_compat.graphql");
pub const JOBS_COMPAT_QUERY: &str = include_str!("weaver/jobs_compat.graphql");
pub const SUBMIT_NZB_MUTATION: &str = include_str!("weaver/submit_nzb.graphql");
pub const SUBMIT_NZB_COMPAT_MUTATION: &str = include_str!("weaver/submit_nzb_compat.graphql");
pub const PAUSE_QUEUE_ITEM_MUTATION: &str = include_str!("weaver/pause_queue_item.graphql");
pub const PAUSE_JOB_MUTATION: &str = include_str!("weaver/pause_job.graphql");
pub const RESUME_QUEUE_ITEM_MUTATION: &str = include_str!("weaver/resume_queue_item.graphql");
pub const RESUME_JOB_MUTATION: &str = include_str!("weaver/resume_job.graphql");
pub const REMOVE_HISTORY_ITEMS_MUTATION: &str = include_str!("weaver/remove_history_items.graphql");
pub const REMOVE_HISTORY_ITEMS_DELETE_FILES_MUTATION: &str =
    include_str!("weaver/remove_history_items_delete_files.graphql");
pub const DELETE_HISTORY_BATCH_MUTATION: &str = include_str!("weaver/delete_history_batch.graphql");
pub const CANCEL_QUEUE_ITEM_MUTATION: &str = include_str!("weaver/cancel_queue_item.graphql");
pub const CANCEL_JOB_MUTATION: &str = include_str!("weaver/cancel_job.graphql");
