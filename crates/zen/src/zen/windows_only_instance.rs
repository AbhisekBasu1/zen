use crate::release_channel::app_identifier;
use util::ResultExt;
use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GENERIC_WRITE, GetLastError, HANDLE},
        Storage::FileSystem::{
            CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_MODE, OPEN_EXISTING,
            PIPE_ACCESS_INBOUND, ReadFile, WriteFile,
        },
        System::{
            Pipes::{
                ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_MESSAGE,
                PIPE_TYPE_MESSAGE, PIPE_WAIT,
            },
            Threading::CreateMutexW,
        },
    },
    core::HSTRING,
};

use crate::{Args, OpenListener, RawOpenRequest};

#[inline]
fn is_first_instance() -> bool {
    unsafe {
        CreateMutexW(
            None,
            false,
            &HSTRING::from(format!("{}-Instance-Mutex", app_identifier())),
        )
        .expect("Unable to create instance mutex.")
    };
    unsafe { GetLastError() != ERROR_ALREADY_EXISTS }
}

pub fn handle_single_instance(opener: OpenListener, args: &Args) -> bool {
    let is_first_instance = is_first_instance();
    if is_first_instance {
        // We are the first instance, listen for messages sent from other instances
        std::thread::Builder::new()
            .name("EnsureSingleton".to_owned())
            .spawn(move || {
                with_pipe(&|url| {
                    opener.open(RawOpenRequest {
                        urls: vec![url],
                        ..Default::default()
                    })
                })
            })
            .unwrap();
    } else if !args.foreground {
        // We are not the first instance, send args to the first instance
        send_args_to_instance(args).log_err();
    }

    is_first_instance
}

fn with_pipe(f: &dyn Fn(String)) {
    let pipe = unsafe {
        CreateNamedPipeW(
            &HSTRING::from(format!("\\\\.\\pipe\\{}-Named-Pipe", app_identifier())),
            PIPE_ACCESS_INBOUND,
            PIPE_TYPE_MESSAGE | PIPE_READMODE_MESSAGE | PIPE_WAIT,
            1,
            128,
            128,
            0,
            None,
        )
    };
    if pipe.is_invalid() {
        log::error!("Failed to create named pipe: {:?}", unsafe {
            GetLastError()
        });
        return;
    }

    loop {
        if let Some(message) = retrieve_message_from_pipe(pipe)
            .context("Failed to read from named pipe")
            .log_err()
        {
            f(message);
        }
    }
}

fn retrieve_message_from_pipe(pipe: HANDLE) -> anyhow::Result<String> {
    unsafe { ConnectNamedPipe(pipe, None)? };
    let message = retrieve_message_from_pipe_inner(pipe);
    unsafe { DisconnectNamedPipe(pipe).log_err() };
    message
}

fn retrieve_message_from_pipe_inner(pipe: HANDLE) -> anyhow::Result<String> {
    let mut buffer = [0u8; 128];
    unsafe {
        ReadFile(pipe, Some(&mut buffer), None, None)?;
    }
    let message = std::ffi::CStr::from_bytes_until_nul(&buffer)?;
    Ok(message.to_string_lossy().into_owned())
}

// This part of code is mostly from crates/cli/src/main.rs
fn send_args_to_instance(args: &Args) -> anyhow::Result<()> {
    if let Some(dock_menu_action_idx) = args.dock_action {
        let url = format!("zen-dock-action://{}", dock_menu_action_idx);
        return write_message_to_instance_pipe(url.as_bytes());
    }

    for path in &args.paths_or_urls {
        let message = if path.starts_with("zen://")
            || path.starts_with("http://")
            || path.starts_with("https://")
            || path.starts_with("file://")
        {
            path.clone()
        } else {
            match std::fs::canonicalize(path) {
                Ok(path) => format!("file://{}", path.to_string_lossy()),
                Err(error) => {
                    log::error!("error parsing path argument: {}", error);
                    continue;
                }
            }
        };
        write_message_to_instance_pipe(message.as_bytes())?;
    }

    Ok(())
}

fn write_message_to_instance_pipe(message: &[u8]) -> anyhow::Result<()> {
    unsafe {
        let pipe = CreateFileW(
            &HSTRING::from(format!("\\\\.\\pipe\\{}-Named-Pipe", app_identifier())),
            GENERIC_WRITE.0,
            FILE_SHARE_MODE::default(),
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES::default(),
            None,
        )?;
        WriteFile(pipe, Some(message), None, None)?;
        CloseHandle(pipe)?;
    }
    Ok(())
}
