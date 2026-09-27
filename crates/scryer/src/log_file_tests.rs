use super::*;
use std::sync::atomic::{AtomicI64, Ordering};
use tracing_subscriber::fmt::MakeWriter;

fn time() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T23:59:59Z")
        .unwrap()
        .with_timezone(&Utc)
}
fn policy(bytes: u64, files: usize) -> LogFilePolicy {
    LogFilePolicy {
        max_bytes: bytes,
        max_files: files,
    }
}
fn clock() -> (Arc<AtomicI64>, Clock) {
    let value = Arc::new(AtomicI64::new(time().timestamp()));
    let clone = value.clone();
    (
        value,
        Arc::new(move || DateTime::from_timestamp(clone.load(Ordering::SeqCst), 0).unwrap()),
    )
}
fn event(writer: &LogFileWriter, value: &str) {
    writer.make_writer().write_all(value.as_bytes()).unwrap();
}
fn idle(writer: &LogFileWriter) {
    let work = writer.0.lock().unwrap().work.clone();
    let queue = work.queue.lock().unwrap();
    let (queue, result) = work
        .changed
        .wait_timeout_while(queue, Duration::from_secs(30), |q| !q.jobs.is_empty())
        .unwrap();
    assert!(
        !result.timed_out() && queue.jobs.is_empty(),
        "compression did not finish"
    );
}
fn gzip(path: &Path) -> String {
    let mut content = String::new();
    flate2::read::GzDecoder::new(File::open(path).unwrap())
        .read_to_string(&mut content)
        .unwrap();
    content
}
fn all(path: &Path) -> Vec<String> {
    let mut contents: Vec<_> = managed_files(path, "gz")
        .unwrap()
        .iter()
        .map(|p| gzip(p))
        .collect();
    contents.push(fs::read_to_string(path).unwrap());
    contents
}
fn segment(path: &Path, sequence: u64) -> PathBuf {
    let mut name = prefix(path);
    name.push(format!(
        "{}-{sequence:020}.sealed",
        time().format("%Y%m%dT%H%M%SZ")
    ));
    path.with_file_name(name)
}

fn work(path: &Path, max_files: usize) -> Work {
    Work {
        queue: Mutex::new(Queue::default()),
        changed: Condvar::new(),
        active_path: path.into(),
        max_files,
        faults: Faults::default(),
    }
}

#[test]
fn policy_defaults_and_strict_positive_overrides() {
    assert_eq!(
        LogFilePolicy::parse(None, None).unwrap(),
        policy(10 * 1024 * 1024, 5)
    );
    assert_eq!(
        LogFilePolicy::parse(Some("12"), Some("2")).unwrap(),
        policy(12, 2)
    );
    for value in [
        "",
        "0",
        "000",
        "-1",
        "+1",
        "1.0",
        " 1",
        "1 ",
        "1MB",
        "18446744073709551616",
    ] {
        assert!(
            LogFilePolicy::parse(Some(value), None)
                .unwrap_err()
                .contains("SCRYER_LOG_MAX_SIZE")
        );
        assert!(
            LogFilePolicy::parse(None, Some(value))
                .unwrap_err()
                .contains("SCRYER_LOG_MAX_FILES")
        );
    }
}

#[test]
fn runtime_size_rotation_preserves_multiline_events_and_appends() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/scryer.log");
    let (_, clock) = clock();
    let (writer, guard) = open_with_clock(&path, policy(4, 20), clock).unwrap();
    {
        let mut record = writer.make_writer();
        record.write_all(b"first\n").unwrap();
        record.write_all(b"continued\n").unwrap();
    }
    assert!(managed_files(&path, "gz").unwrap().is_empty());
    event(&writer, "second\n");
    idle(&writer);
    assert_eq!(all(&path), ["first\ncontinued\n", "second\n"]);
    drop(guard);
    let (writer, guard) = open_log_file(&path, policy(100, 20)).unwrap();
    event(&writer, "third\n");
    drop(guard);
    assert_eq!(fs::read_to_string(path).unwrap(), "second\nthird\n");
}

#[test]
fn utc_rollover_waits_for_an_event_and_never_archives_empty_days() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let (now, clock) = clock();
    let (writer, guard) = open_with_clock(&path, policy(1000, 20), clock).unwrap();
    now.fetch_add(86400 * 5, Ordering::SeqCst);
    assert!(managed_files(&path, "sealed").unwrap().is_empty());
    event(&writer, "a\n");
    assert!(managed_files(&path, "sealed").unwrap().is_empty());
    now.fetch_add(1, Ordering::SeqCst); // Midnight UTC.
    event(&writer, "b\n");
    idle(&writer);
    assert_eq!(all(&path), ["a\n", "b\n"]);
    now.fetch_add(86400 * 3, Ordering::SeqCst);
    event(&writer, "c\n");
    idle(&writer);
    drop(guard);
    assert_eq!(all(&path), ["a\n", "b\n", "c\n"]);
}

#[test]
fn startup_rotates_previous_day_or_oversized_files_on_first_event() {
    for (contents, limit, age) in [("old\n", 100, 86400), ("oversized\n", 2, 0)] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scryer.log");
        fs::write(&path, contents).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified((time() - chrono::Duration::seconds(age)).into())
            .unwrap();
        let (_, clock) = clock();
        let (writer, guard) = open_with_clock(&path, policy(limit, 20), clock).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), contents);
        event(&writer, "new\n");
        idle(&writer);
        drop(guard);
        assert_eq!(all(&path), [contents, "new\n"]);
    }
}

#[test]
fn concurrent_events_appear_exactly_once_without_fragment_interleaving() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let (writer, guard) = open_log_file(&path, policy(100, 1000)).unwrap();
    std::thread::scope(|scope| {
        for thread in 0..8 {
            let writer = writer.clone();
            scope.spawn(move || {
                for record in 0..50 {
                    let mut handle = writer.make_writer();
                    write!(handle, "{thread}:").unwrap();
                    writeln!(handle, "{record}").unwrap();
                }
            });
        }
    });
    idle(&writer);
    drop(guard);
    let content = all(&path).concat();
    let actual: std::collections::BTreeSet<_> = content.lines().map(str::to_owned).collect();
    let expected: std::collections::BTreeSet<_> = (0..8)
        .flat_map(|t| (0..50).map(move |r| format!("{t}:{r}")))
        .collect();
    assert_eq!(content.lines().count(), 400);
    assert_eq!(actual, expected);
}

#[test]
fn retention_only_expires_new_archives_and_counts_archives_not_days() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let preserved = [
        "scryer.log.1.gz",
        "scryer.log.5.gz",
        "scryer.log.1.gz.tmp",
        "unrelated.gz",
        "scryer.log.scryer-rotate-invalid.gz",
    ];
    for name in preserved {
        fs::write(dir.path().join(name), b"preserve").unwrap();
    }
    let (_, clock) = clock();
    let (writer, guard) = open_with_clock(&path, policy(1, 2), clock).unwrap();
    for id in 0..5 {
        event(&writer, &format!("{id}\n"));
        idle(&writer);
    }
    drop(guard);
    assert_eq!(all(&path), ["2\n", "3\n", "4\n"]);
    for name in preserved {
        assert_eq!(fs::read(dir.path().join(name)).unwrap(), b"preserve");
    }
}

#[test]
fn recovery_handles_sealed_and_already_published_segments_without_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    for id in 0..2 {
        let sealed = segment(&path, id);
        fs::write(&sealed, format!("{id}\n")).unwrap();
        if id == 1 {
            publish(&sealed, &Faults::default()).unwrap();
        }
    }
    let (writer, guard) = open_log_file(&path, policy(100, 20)).unwrap();
    idle(&writer);
    event(&writer, "active\n");
    drop(guard);
    assert_eq!(all(&path), ["0\n", "1\n", "active\n"]);
    assert!(managed_files(&path, "sealed").unwrap().is_empty());
}

#[test]
fn compression_and_publication_failures_preserve_source_and_cleanup_owned_temporary() {
    for step in ["compression", "publication"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scryer.log");
        let sealed = segment(&path, 0);
        fs::write(&sealed, "recoverable").unwrap();
        let work = work(&path, 5);
        *work.faults.fail.lock().unwrap() = Some(step);
        let unrelated = sealed.with_extension("old.tmp");
        fs::write(&unrelated, "unknown owner").unwrap();
        let mut job = Job {
            source: Some(sealed.clone()),
        };
        assert!(process_job(&mut job, &work).is_err());
        assert_eq!(fs::read_to_string(&sealed).unwrap(), "recoverable");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
        process_job(&mut job, &work).unwrap();
        assert_eq!(gzip(&sealed.with_extension("gz")), "recoverable");
        assert!(job.source.is_none());
        assert!(unrelated.exists());
    }
}

#[test]
fn retention_failure_retries_without_recompressing_or_losing_the_published_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let work = work(&path, 1);
    for id in 0..2 {
        let sealed = segment(&path, id);
        fs::write(&sealed, format!("{id}")).unwrap();
        let mut job = Job {
            source: Some(sealed.clone()),
        };
        if id == 1 {
            *work.faults.fail.lock().unwrap() = Some("retention");
            assert!(process_job(&mut job, &work).is_err());
            assert!(job.source.is_none());
            assert_eq!(managed_files(&path, "gz").unwrap().len(), 2);
        }
        process_job(&mut job, &work).unwrap();
    }
    assert_eq!(gzip(&segment(&path, 1).with_extension("gz")), "1");
    assert_eq!(managed_files(&path, "gz").unwrap().len(), 1);
}

#[test]
fn archive_collision_preserves_both_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let sealed = segment(&path, 0);
    fs::write(&sealed, "source").unwrap();
    let archive = sealed.with_extension("gz");
    fs::write(&archive, "unrelated").unwrap();
    assert!(publish(&sealed, &Faults::default()).is_err());
    assert_eq!(fs::read_to_string(sealed).unwrap(), "source");
    assert_eq!(fs::read_to_string(archive).unwrap(), "unrelated");
}

#[test]
fn rename_and_reopen_failures_preserve_event_contents() {
    for step in ["rename", "reopen"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("scryer.log");
        let (writer, guard) = open_log_file(&path, policy(1, 20)).unwrap();
        event(&writer, "old\n");
        *writer.0.lock().unwrap().work.faults.fail.lock().unwrap() = Some(step);
        event(&writer, "new\n");
        idle(&writer);
        drop(guard);
        assert_eq!(all(&path).concat(), "old\nnew\n");
    }
}

#[test]
fn one_pending_segment_defers_rollover_but_not_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let (writer, guard) = open_log_file(&path, policy(1, 20)).unwrap();
    // Stop the worker, then install a pending job under explicit test control.
    drop(guard);
    let work = writer.0.lock().unwrap().work.clone();
    {
        let mut queue = work.queue.lock().unwrap();
        queue.shutdown = false;
        queue.jobs.push_back(Job {
            source: Some(segment(&path, 0)),
        });
    }
    event(&writer, "a\n");
    event(&writer, "b\n");
    event(&writer, "c\n");
    assert_eq!(work.queue.lock().unwrap().jobs.len(), 1);
    assert_eq!(fs::read_to_string(path).unwrap(), "a\nb\nc\n");
}

#[cfg(unix)]
#[test]
fn symlinks_are_not_followed_or_retained_as_managed_archives() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let unrelated = dir.path().join("unrelated");
    fs::write(&unrelated, "preserve").unwrap();
    let sealed = segment(&path, 0);
    symlink(&unrelated, &sealed).unwrap();
    assert!(managed_files(&path, "sealed").unwrap().is_empty());
    assert!(open_regular(&sealed).is_err());
    let archive = segment(&path, 1).with_extension("gz");
    symlink(&unrelated, &archive).unwrap();
    assert!(managed_files(&path, "gz").unwrap().is_empty());
    symlink(&unrelated, &path).unwrap();
    assert!(open_log_file(&path, policy(1, 1)).is_err());
    assert_eq!(fs::read_to_string(unrelated).unwrap(), "preserve");
}

#[test]
fn restart_preserves_sequence_order_and_retries_pending_retention() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let (now, clock) = clock();
    let (writer, guard) = open_with_clock(&path, policy(1, 20), clock.clone()).unwrap();
    for value in ["a\n", "b\n", "c\n"] {
        event(&writer, value);
        idle(&writer);
    }
    drop(guard);
    drop(writer);
    now.fetch_sub(86400, Ordering::SeqCst);
    let (writer, guard) = open_with_clock(&path, policy(1, 1), clock).unwrap();
    idle(&writer); // Reducing retention also works before another rollover.
    assert_eq!(managed_files(&path, "gz").unwrap().len(), 1);
    event(&writer, "d\n");
    drop(guard); // Joins the worker without a separate wait for compression.
    assert_eq!(all(&path), ["c\n", "d\n"]);
}

#[test]
fn occupied_rotation_name_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let (_, clock) = clock();
    let (writer, guard) = open_with_clock(&path, policy(1, 20), clock).unwrap();
    let occupied = segment(&path, 0);
    fs::write(&occupied, "not ours").unwrap();
    event(&writer, "a\n");
    event(&writer, "b\n");
    idle(&writer);
    drop(guard);
    assert_eq!(fs::read_to_string(occupied).unwrap(), "not ours");
    assert_eq!(all(&path), ["a\n", "b\n"]);
}

#[cfg(unix)]
#[test]
fn non_utf8_active_names_cannot_share_an_archive_namespace() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(std::ffi::OsString::from_vec(b"log-\xff".to_vec()));
    let unrelated = dir
        .path()
        .join("log-�.scryer-rotate-20260101T235959Z-00000000000000000000.gz");
    fs::write(&unrelated, "preserve").unwrap();
    assert!(managed_files(&path, "gz").unwrap().is_empty());
    // Linux permits these names; macOS filesystems reject their creation.
    #[cfg(target_os = "linux")]
    {
        let (writer, guard) = open_log_file(&path, policy(1, 1)).unwrap();
        event(&writer, "a\n");
        event(&writer, "b\n");
        idle(&writer);
        drop(guard);
        assert_eq!(all(&path), ["a\n", "b\n"]);
    }
    assert_eq!(fs::read_to_string(unrelated).unwrap(), "preserve");
}

#[test]
fn interrupted_empty_reservation_does_not_create_an_empty_archive() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let reservation = segment(&path, 0);
    fs::write(&reservation, "").unwrap();
    let (writer, guard) = open_log_file(&path, policy(1, 1)).unwrap();
    idle(&writer);
    drop(guard);
    assert!(reservation.exists());
    assert!(managed_files(&path, "gz").unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn rotation_preserves_private_active_and_archive_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    fs::write(&path, "private\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let (writer, guard) = open_log_file(&path, policy(1, 5)).unwrap();
    event(&writer, "new private\n");
    idle(&writer);
    drop(guard);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let archives = managed_files(&path, "gz").unwrap();
    assert_eq!(archives.len(), 1);
    assert_eq!(
        fs::metadata(&archives[0]).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(all(&path), ["private\n", "new private\n"]);
}

#[test]
fn unsupported_hard_links_do_not_stall_subsequent_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("scryer.log");
    let (writer, guard) = open_log_file(&path, policy(1, 5)).unwrap();
    event(&writer, "a\n");
    for record in ["b\n", "c\n"] {
        *writer.0.lock().unwrap().work.faults.fail.lock().unwrap() = Some("hard_link");
        event(&writer, record);
        idle(&writer);
    }
    drop(guard);
    assert_eq!(all(&path), ["a\n", "b\n", "c\n"]);
    assert!(managed_files(&path, "sealed").unwrap().is_empty());
}

#[test]
fn exclusive_rename_fallback_never_replaces_an_occupied_destination() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("owned.tmp");
    let destination = dir.path().join("archive.gz");
    fs::write(&source, "ours").unwrap();
    fs::write(&destination, "theirs").unwrap();
    assert!(publish_platform::rename_exclusive(&source, &destination).is_err());
    assert_eq!(fs::read_to_string(&source).unwrap(), "ours");
    assert_eq!(fs::read_to_string(&destination).unwrap(), "theirs");
}

#[cfg(unix)]
#[test]
fn exclusive_rename_fallback_preserves_a_dangling_symlink_collision() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("owned.tmp");
    let destination = dir.path().join("archive.gz");
    fs::write(&source, "ours").unwrap();
    symlink("missing", &destination).unwrap();
    assert!(publish_platform::rename_exclusive(&source, &destination).is_err());
    assert_eq!(fs::read_to_string(&source).unwrap(), "ours");
    assert_eq!(fs::read_link(&destination).unwrap(), Path::new("missing"));
}
