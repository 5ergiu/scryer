use super::support_bootstrap_fixtures::bootstrap;
use crate::start_background_custom_job_scheduler;
use crate::test_wait::within_deadline;

#[tokio::test]
async fn stopping_the_scheduler_cancels_running_scheduled_scripts() {
    let (app, _user) = bootstrap();
    let script_shutdown = app.runtime.jobs.custom_job_shutdown.clone();
    assert!(!script_shutdown.is_cancelled());

    let token = tokio_util::sync::CancellationToken::new();
    token.cancel();
    within_deadline(
        "the scheduler to stop",
        start_background_custom_job_scheduler(app, token),
    )
    .await;

    assert!(
        script_shutdown.is_cancelled(),
        "a stopped scheduler stops the scripts it started"
    );
}
