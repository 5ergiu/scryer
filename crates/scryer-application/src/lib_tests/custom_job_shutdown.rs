use super::support_bootstrap_fixtures::bootstrap;
use crate::start_background_custom_job_scheduler;
use crate::test_wait::within_deadline;

#[tokio::test]
async fn stopping_the_scheduler_cancels_running_scheduled_scripts() {
    let (app, _user) = bootstrap();
    let script_shutdown = app.runtime.jobs.script_shutdown.clone();
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

#[tokio::test]
async fn shutting_down_scripts_cancels_and_waits_for_tracked_runs() {
    let (app, _user) = bootstrap();
    let jobs = &app.runtime.jobs;
    let seen_cancel = jobs.script_shutdown.clone();
    // A stand-in for a running script: ends only once shutdown cancels it.
    let run = jobs
        .script_runs
        .spawn(async move { seen_cancel.cancelled().await });
    assert_eq!(jobs.script_runs.len(), 1);

    within_deadline("running scripts to stop", app.shutdown_scripts()).await;

    assert!(jobs.script_shutdown.is_cancelled());
    assert!(jobs.script_runs.is_closed());
    assert_eq!(jobs.script_runs.len(), 0, "shutdown waited for the run");
    within_deadline("the run task to join", run)
        .await
        .expect("the run ended on cancellation");
}
