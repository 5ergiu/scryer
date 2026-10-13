//! Own a script's Windows process tree before its first instruction runs.

use std::io;
use std::mem::size_of;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};

pub(super) struct ProcessGroup(OwnedHandle);

impl ProcessGroup {
    pub(super) fn new() -> io::Result<Self> {
        // An unnamed, non-inheritable job. Only this guard owns its handle.
        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error());
        }
        let job = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // The buffer is the documented structure for this information class.
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(job))
    }

    /// The caller must spawn with CREATE_SUSPENDED and kill_on_drop enabled.
    /// Assignment happens before resuming, so no descendant can escape the job.
    pub(super) fn attach_and_resume(&self, child: &tokio::process::Child) -> io::Result<()> {
        let process = child
            .raw_handle()
            .ok_or_else(|| io::Error::other("script exited before job assignment"))?;
        if unsafe { AssignProcessToJobObject(self.0.as_raw_handle(), process) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("script has no process id"))?;
        // Tokio retains the process handle but not CreateProcess's primary
        // thread handle. A suspended new process has only its initial thread;
        // discover that thread without allowing the process to execute first.
        let raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if raw == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut entry = THREADENTRY32 {
            dwSize: size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut found = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
        while found != 0 {
            if entry.th32OwnerProcessID == pid {
                let raw = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if raw.is_null() {
                    return Err(io::Error::last_os_error());
                }
                let thread = unsafe { OwnedHandle::from_raw_handle(raw) };
                if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
                    return Err(io::Error::last_os_error());
                }
                return Ok(());
            }
            entry.dwSize = size_of::<THREADENTRY32>() as u32;
            found = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
        }
        Err(io::Error::other("suspended script thread was not found"))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{
        InterpreterConfig, ScriptInvocation, ScriptOutcome, ScriptSource, run_script_cancellable,
    };
    use super::*;
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio_util::sync::CancellationToken;
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, SYNCHRONIZATION_SYNCHRONIZE, WaitForSingleObject,
    };

    const BOUND: Duration = Duration::from_secs(60);

    async fn stops_descendant(mode: &str, capture_output: bool) {
        let fixture = tempfile::tempdir().expect("script fixture");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // Report the descendant's PID only after it exists, then block on an
        // explicit message. Neither readiness nor termination uses polling.
        let script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$child = Start-Process powershell.exe -ArgumentList '-NoProfile','-NonInteractive','-Command','Start-Sleep -Seconds 600' -PassThru
$client = [System.Net.Sockets.TcpClient]::new('127.0.0.1', {port})
$stream = $client.GetStream()
$bytes = [System.Text.Encoding]::ASCII.GetBytes([string]$child.Id + "`n")
$stream.Write($bytes, 0, $bytes.Length)
$null = $stream.ReadByte()
$client.Dispose()
"#
        );
        let invocation = ScriptInvocation {
            script_id: "windows-tree".into(),
            source: ScriptSource::Inline {
                content: script,
                language: scryer_domain::ScriptLanguage::PowerShell,
            },
            env: vec![],
            cwd: fixture.path().into(),
            timeout: Duration::from_secs(600),
            capture_output,
            interpreters: InterpreterConfig {
                powershell: Some("powershell.exe".into()),
                ..Default::default()
            },
            materialize_root: fixture.path().into(),
            terminate_grace: Duration::ZERO,
        };
        let cancel = CancellationToken::new();
        let task_cancel = cancel.clone();
        let run =
            tokio::spawn(async move { run_script_cancellable(invocation, &task_cancel).await });
        let (stream, _) = tokio::time::timeout(BOUND, listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut stream = BufReader::new(stream);
        let mut pid = String::new();
        tokio::time::timeout(BOUND, stream.read_line(&mut pid))
            .await
            .unwrap()
            .unwrap();
        let raw =
            unsafe { OpenProcess(SYNCHRONIZATION_SYNCHRONIZE, 0, pid.trim().parse().unwrap()) };
        assert!(!raw.is_null(), "descendant is alive before ending the run");
        let descendant = unsafe { OwnedHandle::from_raw_handle(raw) };
        match mode {
            "timeout" => {
                tokio::time::pause();
                tokio::time::advance(Duration::from_secs(601)).await;
                tokio::time::resume();
            }
            "cancel" => cancel.cancel(),
            "abort" => run.abort(),
            "exit" => {
                tokio::time::timeout(BOUND, stream.get_mut().write_all(&[1]))
                    .await
                    .unwrap()
                    .unwrap();
            }
            _ => unreachable!(),
        }
        let result = tokio::time::timeout(BOUND, run)
            .await
            .expect("runner finishes");
        match mode {
            "timeout" => assert_eq!(result.unwrap().outcome, ScriptOutcome::TimedOut),
            "cancel" => assert_eq!(result.unwrap().outcome, ScriptOutcome::Cancelled),
            "abort" => assert!(result.unwrap_err().is_cancelled()),
            "exit" => assert!(matches!(
                result.unwrap().outcome,
                ScriptOutcome::Exited { success: true, .. }
            )),
            _ => unreachable!(),
        }
        let terminated = tokio::task::spawn_blocking(move || unsafe {
            WaitForSingleObject(descendant.as_raw_handle(), 60_000)
        });
        assert_eq!(
            tokio::time::timeout(BOUND + Duration::from_secs(5), terminated)
                .await
                .unwrap()
                .unwrap(),
            WAIT_OBJECT_0
        );
    }

    #[tokio::test]
    async fn timeout_kills_descendants_with_and_without_output_capture() {
        for capture in [true, false] {
            stops_descendant("timeout", capture).await;
        }
    }
    #[tokio::test]
    async fn cancellation_kills_descendants() {
        stops_descendant("cancel", true).await;
    }
    #[tokio::test]
    async fn aborting_the_runner_kills_descendants() {
        stops_descendant("abort", true).await;
    }
    #[tokio::test]
    async fn interpreter_exit_does_not_leave_descendants() {
        stops_descendant("exit", true).await;
    }
}
