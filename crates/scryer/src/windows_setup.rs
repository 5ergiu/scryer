//! Double-click setup entrypoint. Recovery completes before MSI execution starts.

use std::os::windows::ffi::OsStrExt;
use std::ptr::{null, null_mut};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

fn message(text: &str, flags: u32) {
    // SAFETY: Strings remain alive for the duration of the modal call.
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(text).as_ptr(),
            wide("Scryer Setup").as_ptr(),
            flags,
        );
    }
}

pub fn run(install: fn() -> Result<bool, String>) {
    let elevated = std::env::args_os().skip(1).collect::<Vec<_>>() == ["--elevated"];
    if !elevated {
        let result = std::env::current_exe().map(|path| {
            // SAFETY: ShellExecute copies the strings before returning. The
            // recovery operation independently verifies Administrator membership.
            unsafe {
                ShellExecuteW(
                    null_mut(),
                    wide("runas").as_ptr(),
                    wide(path).as_ptr(),
                    wide("--elevated").as_ptr(),
                    null(),
                    SW_SHOWNORMAL,
                ) as isize
            }
        });
        if !matches!(result, Ok(code) if code > 32) {
            message(
                "Setup was not started. Open the download again and approve the Windows permission prompt when you are ready.",
                MB_OK | MB_ICONINFORMATION,
            );
        }
        return;
    }
    // A simple native status window stays responsive while recovery downloads
    // and verifies the original installer. MSI provides its own progress UI.
    // There is no close button while Windows Installer may be servicing files.
    let window = unsafe {
        CreateWindowExW(0, wide("STATIC").as_ptr(), wide("Preparing Scryer. Restoring installer information and verifying downloads. Please wait...").as_ptr(), WS_OVERLAPPED | WS_CAPTION | WS_VISIBLE, CW_USEDEFAULT, CW_USEDEFAULT, 560, 130, null_mut(), null_mut(), null_mut(), null())
    };
    if window.is_null() {
        message(
            "Scryer Setup could not open its progress window. Please try again.",
            MB_OK | MB_ICONERROR,
        );
        return;
    }
    let thread_id = unsafe { GetCurrentThreadId() };
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let result = std::panic::catch_unwind(install)
            .unwrap_or_else(|_| Err("Setup encountered an unexpected error.".into()));
        let _ = sender.send(result);
        // SAFETY: The UI thread owns the message queue and remains alive until
        // it receives this completion message.
        unsafe {
            PostThreadMessageW(thread_id, WM_APP, 0, 0);
        }
    });
    let mut msg: MSG = unsafe { std::mem::zeroed() };
    // SAFETY: Window creation initialized this thread's queue. No pointers are
    // passed through the completion message; results use the channel.
    unsafe {
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.message == WM_APP {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        DestroyWindow(window);
    }
    match receiver.recv() {
        Ok(Ok(true)) => message(
            "Scryer was installed. Restart Windows to finish the upgrade.",
            MB_OK | MB_ICONINFORMATION,
        ),
        Ok(Ok(false)) => message(
            "Scryer was installed. Open Scryer from the Start menu.",
            MB_OK | MB_ICONINFORMATION,
        ),
        Ok(Err(error)) => message(
            &format!("Setup could not finish. Please share this message with support.\n\n{error}"),
            MB_OK | MB_ICONERROR,
        ),
        Err(_) => message(
            "Setup could not finish. Please try again.",
            MB_OK | MB_ICONERROR,
        ),
    }
}
