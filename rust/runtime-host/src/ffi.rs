use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::engine::NullEngineAdapter;
use crate::host::RuntimeHostCore;
use crate::protocol::{
    ArchiveStats, HostContextFsCommand, HostFsCommand, HostFsResponse, HostProcessInfo,
    HostRuntimeBindings, HostRuntimeBuiltinSpec, HostRuntimeCommand, HostRuntimeContext,
    HostRuntimeResponse, HostRuntimeTimer, HostRuntimeTimerKind, RunRequest,
};
use crate::vfs::VirtualFile;

thread_local! {
    static HOST: RefCell<RuntimeHostCore<NullEngineAdapter>> =
        RefCell::new(RuntimeHostCore::new(NullEngineAdapter));
    static LAST_RESULT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }

    let mut bytes = vec![0_u8; len];
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    ptr
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn runtime_host_dealloc(ptr: *mut u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }

    unsafe {
        drop(Vec::from_raw_parts(ptr, len, len));
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_last_result_ptr() -> *const u8 {
    LAST_RESULT.with(|result| result.borrow().as_ptr())
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_last_result_len() -> usize {
    LAST_RESULT.with(|result| result.borrow().len())
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_boot_summary_json() -> u32 {
    HOST.with(|host| {
        let summary = host.borrow().boot_summary();
        set_last_result(render_boot_summary_json(&summary));
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_create_session_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_else(|| "session".into());
    let archive_file_name =
        required_field(&fields, "archive_file_name").unwrap_or_else(|| "guest.zip".into());
    let package_name = optional_field(&fields, "package_name");
    let package_scripts = match parse_string_map_field(&fields, "package_scripts") {
        Ok(scripts) => scripts,
        Err(error) => return write_error(error),
    };
    let files = match parse_virtual_files(&fields) {
        Ok(files) => files,
        Err(error) => return write_error(error),
    };
    let archive = ArchiveStats {
        file_name: archive_file_name,
        file_count: parse_usize_field(&fields, "file_count"),
        directory_count: parse_usize_field(&fields, "directory_count"),
        root_prefix: optional_field(&fields, "root_prefix"),
    };

    HOST.with(|host| {
        let result = host.borrow_mut().create_session_with_id(
            session_id,
            archive,
            package_name,
            package_scripts,
            files,
        );

        match result {
            Ok(snapshot) => set_last_result(render_session_handle_json(&snapshot)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_plan_run_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let cwd = required_field(&fields, "cwd").unwrap_or_else(|| "/workspace".into());
    let command = required_field(&fields, "command").unwrap_or_default();
    let args = fields
        .get("args")
        .map(|value| {
            value
                .split('\u{1f}')
                .filter(|segment| !segment.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let env = fields
        .get("env")
        .map(|value| {
            value
                .split('\u{1f}')
                .filter_map(|entry| entry.split_once('='))
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    HOST.with(|host| {
        let result = host.borrow().plan_run(
            &session_id,
            &RunRequest {
                cwd,
                command,
                args,
                env,
            },
        );

        match result {
            Ok(plan) => set_last_result(render_run_plan_json(&plan)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_build_process_info_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let cwd = required_field(&fields, "cwd").unwrap_or_else(|| "/workspace".into());
    let command = required_field(&fields, "command").unwrap_or_default();
    let args = fields
        .get("args")
        .map(|value| {
            value
                .split('\u{1f}')
                .filter(|segment| !segment.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let env = fields
        .get("env")
        .map(|value| {
            value
                .split('\u{1f}')
                .filter_map(|entry| entry.split_once('='))
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    HOST.with(|host| {
        let result = host.borrow().build_process_info(
            &session_id,
            &RunRequest {
                cwd,
                command,
                args,
                env,
            },
        );

        match result {
            Ok(process_info) => set_last_result(render_process_info_json(&process_info)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_create_runtime_context_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let cwd = required_field(&fields, "cwd").unwrap_or_else(|| "/workspace".into());
    let command = required_field(&fields, "command").unwrap_or_default();
    let args = fields
        .get("args")
        .map(|value| {
            value
                .split('\u{1f}')
                .filter(|segment| !segment.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let env = fields
        .get("env")
        .map(|value| {
            value
                .split('\u{1f}')
                .filter_map(|entry| entry.split_once('='))
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();

    HOST.with(|host| {
        let result = host.borrow_mut().create_runtime_context(
            &session_id,
            &RunRequest {
                cwd,
                command,
                args,
                env,
            },
        );

        match result {
            Ok(context) => set_last_result(render_runtime_context_json(&context)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_stop_session_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();

    HOST.with(|host| {
        let result = host.borrow_mut().stop_session(&session_id);

        match result {
            Ok(()) => set_last_result(format!(
                "{{\"sessionId\":\"{}\"}}",
                escape_json(&session_id)
            )),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_list_workspace_files_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();

    HOST.with(|host| {
        let result = host.borrow().workspace_file_summaries(&session_id);

        match result {
            Ok(files) => set_last_result(render_workspace_file_summaries_json(&files)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_read_workspace_file_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let encoded_path = required_field(&fields, "path").unwrap_or_default();
    let path = match decode_hex(&encoded_path) {
        Ok(path) => path,
        Err(error) => return write_error(error),
    };

    HOST.with(|host| {
        let result = host.borrow().read_workspace_file(&session_id, &path);

        match result {
            Ok(file) => set_last_result(render_workspace_file_json(&file)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_read_workspace_files_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let encoded_paths = fields.get("paths").cloned().unwrap_or_default();
    let paths = match parse_hex_path_list(&encoded_paths) {
        Ok(paths) => paths,
        Err(error) => return write_error(error),
    };

    HOST.with(|host| {
        let host = host.borrow();
        let mut files = Vec::with_capacity(paths.len());

        for path in paths {
            match host.read_workspace_file(&session_id, &path) {
                Ok(file) => files.push(file),
                Err(error) => {
                    set_last_result(render_error_json(&error.to_string()));
                    return;
                }
            }
        }

        set_last_result(render_workspace_files_json(&files));
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_stat_workspace_path_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let path = required_field(&fields, "path").unwrap_or_else(|| "/workspace".into());

    HOST.with(|host| {
        let result = host.borrow().stat_workspace_path(&session_id, &path);

        match result {
            Ok(entry) => set_last_result(render_workspace_entry_json(&entry)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_read_workspace_directory_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let path = required_field(&fields, "path").unwrap_or_else(|| "/workspace".into());

    HOST.with(|host| {
        let result = host.borrow().read_workspace_directory(&session_id, &path);

        match result {
            Ok(entries) => set_last_result(render_workspace_entries_json(&entries)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_create_workspace_directory_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let encoded_path = required_field(&fields, "path").unwrap_or_default();
    let path = match decode_hex(&encoded_path) {
        Ok(path) => path,
        Err(error) => return write_error(error),
    };

    HOST.with(|host| {
        let result = host
            .borrow_mut()
            .create_workspace_directory(&session_id, &path);

        match result {
            Ok(entry) => set_last_result(render_workspace_entry_json(&entry)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_write_workspace_file_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let encoded_path = required_field(&fields, "path").unwrap_or_default();
    let path = match decode_hex(&encoded_path) {
        Ok(path) => path,
        Err(error) => return write_error(error),
    };
    let is_text = fields
        .get("is_text")
        .map(|value| value == "true" || value == "1")
        .unwrap_or(false);
    let encoded_bytes = required_field(&fields, "bytes").unwrap_or_default();
    let bytes = match decode_hex_bytes(&encoded_bytes) {
        Ok(bytes) => bytes,
        Err(error) => return write_error(error),
    };

    HOST.with(|host| {
        let result = host
            .borrow_mut()
            .write_workspace_file(&session_id, &path, bytes, is_text);

        match result {
            Ok(entry) => set_last_result(render_workspace_entry_json(&entry)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_execute_fs_command_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let command = match parse_fs_command(&fields) {
        Ok(command) => command,
        Err(error) => return write_error(error),
    };

    HOST.with(|host| {
        let result = host.borrow_mut().execute_fs_command(&session_id, command);

        match result {
            Ok(response) => set_last_result(render_fs_response_json(&response)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_execute_context_fs_command_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let context_id = required_field(&fields, "context_id").unwrap_or_default();
    let command = match parse_context_fs_command(&fields) {
        Ok(command) => command,
        Err(error) => return write_error(error),
    };

    HOST.with(|host| {
        let result = host
            .borrow_mut()
            .execute_context_fs_command(&context_id, command);

        match result {
            Ok(response) => set_last_result(render_fs_response_json(&response)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_execute_runtime_command_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let context_id = required_field(&fields, "context_id").unwrap_or_default();
    let command = match parse_runtime_command(&fields) {
        Ok(command) => command,
        Err(error) => return write_error(error),
    };

    HOST.with(|host| {
        let result = host
            .borrow_mut()
            .execute_runtime_command(&context_id, command);

        match result {
            Ok(response) => set_last_result(render_runtime_response_json(&response)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_drop_runtime_context_json(ptr: *const u8, len: usize) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let context_id = required_field(&fields, "context_id").unwrap_or_default();

    HOST.with(|host| {
        let result = host.borrow_mut().drop_runtime_context(&context_id);

        match result {
            Ok(()) => set_last_result(format!(
                "{{\"contextId\":\"{}\"}}",
                escape_json(&context_id)
            )),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

#[unsafe(no_mangle)]
pub extern "C" fn runtime_host_resolve_preview_request_hint_json(
    ptr: *const u8,
    len: usize,
) -> u32 {
    let input = match read_input(ptr, len) {
        Ok(input) => input,
        Err(error) => return write_error(error),
    };

    let fields = parse_fields(&input);
    let session_id = required_field(&fields, "session_id").unwrap_or_default();
    let relative_path = match required_field(&fields, "relative_path") {
        Some(path) => match decode_hex(&path) {
            Ok(path) => path,
            Err(error) => return write_error(error),
        },
        None => "/".into(),
    };

    HOST.with(|host| {
        let result = host
            .borrow()
            .resolve_preview_request_hint(&session_id, &relative_path);

        match result {
            Ok(hint) => set_last_result(render_preview_request_hint_json(&hint)),
            Err(error) => set_last_result(render_error_json(&error.to_string())),
        }
    });

    1
}

fn read_input(ptr: *const u8, len: usize) -> Result<String, String> {
    if ptr.is_null() || len == 0 {
        return Ok(String::new());
    }

    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string())
}

fn parse_fields(input: &str) -> BTreeMap<String, String> {
    input
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn required_field(fields: &BTreeMap<String, String>, key: &str) -> Option<String> {
    fields.get(key).cloned().filter(|value| !value.is_empty())
}

fn optional_field(fields: &BTreeMap<String, String>, key: &str) -> Option<String> {
    fields.get(key).cloned().filter(|value| !value.is_empty())
}

fn parse_usize_field(fields: &BTreeMap<String, String>, key: &str) -> usize {
    fields
        .get(key)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
}

fn parse_u64_field(fields: &BTreeMap<String, String>, key: &str) -> u64 {
    fields
        .get(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0)
}

fn parse_virtual_files(fields: &BTreeMap<String, String>) -> Result<Vec<VirtualFile>, String> {
    let Some(encoded) = fields.get("files") else {
        return Ok(Vec::new());
    };

    if encoded.is_empty() {
        return Ok(Vec::new());
    }

    encoded
        .split('\u{1e}')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let mut segments = entry.split('\u{1f}');
            let path_hex = segments
                .next()
                .ok_or_else(|| "missing file path".to_string())?;
            let is_text = segments
                .next()
                .ok_or_else(|| "missing file kind".to_string())?;
            let bytes_hex = segments
                .next()
                .ok_or_else(|| "missing file bytes".to_string())?;

            Ok(VirtualFile {
                path: decode_hex(path_hex)?,
                bytes: decode_hex_bytes(bytes_hex)?,
                is_text: is_text == "1",
            })
        })
        .collect()
}

fn parse_fs_command(fields: &BTreeMap<String, String>) -> Result<HostFsCommand, String> {
    let kind = required_field(fields, "command").ok_or_else(|| "missing fs command".to_string())?;
    let cwd = match required_field(fields, "cwd") {
        Some(encoded) => decode_hex(&encoded)?,
        None => "/workspace".into(),
    };
    let path = match required_field(fields, "path") {
        Some(encoded) => decode_hex(&encoded)?,
        None => "/workspace".into(),
    };

    match kind.as_str() {
        "exists" => Ok(HostFsCommand::Exists { cwd, path }),
        "stat" => Ok(HostFsCommand::Stat { cwd, path }),
        "read-dir" => Ok(HostFsCommand::ReadDir { cwd, path }),
        "read-file" => Ok(HostFsCommand::ReadFile { cwd, path }),
        "mkdir" => Ok(HostFsCommand::CreateDirAll { cwd, path }),
        "write-file" => {
            let is_text = fields
                .get("is_text")
                .map(|value| value == "true" || value == "1")
                .unwrap_or(false);
            let encoded_bytes = required_field(fields, "bytes").unwrap_or_default();
            let bytes = decode_hex_bytes(&encoded_bytes)?;

            Ok(HostFsCommand::WriteFile {
                cwd,
                path,
                bytes,
                is_text,
            })
        }
        _ => Err(format!("unsupported fs command: {kind}")),
    }
}

fn parse_context_fs_command(
    fields: &BTreeMap<String, String>,
) -> Result<HostContextFsCommand, String> {
    let kind = required_field(fields, "command").ok_or_else(|| "missing fs command".to_string())?;
    let path = match required_field(fields, "path") {
        Some(encoded) => decode_hex(&encoded)?,
        None => ".".into(),
    };

    match kind.as_str() {
        "exists" => Ok(HostContextFsCommand::Exists { path }),
        "stat" => Ok(HostContextFsCommand::Stat { path }),
        "read-dir" => Ok(HostContextFsCommand::ReadDir { path }),
        "read-file" => Ok(HostContextFsCommand::ReadFile { path }),
        "mkdir" => Ok(HostContextFsCommand::CreateDirAll { path }),
        "write-file" => {
            let is_text = fields
                .get("is_text")
                .map(|value| value == "true" || value == "1")
                .unwrap_or(false);
            let encoded_bytes = required_field(fields, "bytes").unwrap_or_default();
            let bytes = decode_hex_bytes(&encoded_bytes)?;

            Ok(HostContextFsCommand::WriteFile {
                path,
                bytes,
                is_text,
            })
        }
        _ => Err(format!("unsupported fs command: {kind}")),
    }
}

fn parse_runtime_command(fields: &BTreeMap<String, String>) -> Result<HostRuntimeCommand, String> {
    let kind =
        required_field(fields, "command").ok_or_else(|| "missing runtime command".to_string())?;

    match kind.as_str() {
        "runtime-describe" => Ok(HostRuntimeCommand::DescribeBindings),
        "timers-schedule" => Ok(HostRuntimeCommand::TimerSchedule {
            delay_ms: parse_u64_field(fields, "delay_ms"),
            repeat: fields
                .get("repeat")
                .map(|value| value == "true" || value == "1")
                .unwrap_or(false),
        }),
        "timers-clear" => Ok(HostRuntimeCommand::TimerClear {
            timer_id: required_field(fields, "timer_id").unwrap_or_default(),
        }),
        "timers-list" => Ok(HostRuntimeCommand::TimerList),
        "timers-advance" => Ok(HostRuntimeCommand::TimerAdvance {
            elapsed_ms: parse_u64_field(fields, "elapsed_ms"),
        }),
        "process-info" => Ok(HostRuntimeCommand::ProcessInfo),
        "process-cwd" => Ok(HostRuntimeCommand::ProcessCwd),
        "process-argv" => Ok(HostRuntimeCommand::ProcessArgv),
        "process-env" => Ok(HostRuntimeCommand::ProcessEnv),
        "process-chdir" => Ok(HostRuntimeCommand::ProcessChdir {
            path: match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            },
        }),
        "path-resolve" => Ok(HostRuntimeCommand::PathResolve {
            segments: parse_hex_path_list(
                fields.get("segments").map(String::as_str).unwrap_or(""),
            )?,
        }),
        "path-join" => Ok(HostRuntimeCommand::PathJoin {
            segments: parse_hex_path_list(
                fields.get("segments").map(String::as_str).unwrap_or(""),
            )?,
        }),
        "path-dirname" => Ok(HostRuntimeCommand::PathDirname {
            path: match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            },
        }),
        "path-basename" => Ok(HostRuntimeCommand::PathBasename {
            path: match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            },
        }),
        "path-extname" => Ok(HostRuntimeCommand::PathExtname {
            path: match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            },
        }),
        "path-normalize" => Ok(HostRuntimeCommand::PathNormalize {
            path: match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            },
        }),
        "fs-exists" => Ok(HostRuntimeCommand::Fs(HostContextFsCommand::Exists {
            path: match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            },
        })),
        "fs-stat" => Ok(HostRuntimeCommand::Fs(HostContextFsCommand::Stat {
            path: match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            },
        })),
        "fs-read-dir" => Ok(HostRuntimeCommand::Fs(HostContextFsCommand::ReadDir {
            path: match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            },
        })),
        "fs-read-file" => Ok(HostRuntimeCommand::Fs(HostContextFsCommand::ReadFile {
            path: match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            },
        })),
        "fs-mkdir" => Ok(HostRuntimeCommand::Fs(HostContextFsCommand::CreateDirAll {
            path: match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            },
        })),
        "fs-write-file" => {
            let path = match required_field(fields, "path") {
                Some(encoded) => decode_hex(&encoded)?,
                None => ".".into(),
            };
            let is_text = fields
                .get("is_text")
                .map(|value| value == "true" || value == "1")
                .unwrap_or(false);
            let encoded_bytes = required_field(fields, "bytes").unwrap_or_default();
            let bytes = decode_hex_bytes(&encoded_bytes)?;

            Ok(HostRuntimeCommand::Fs(HostContextFsCommand::WriteFile {
                path,
                bytes,
                is_text,
            }))
        }
        _ => Err(format!("unsupported runtime command: {kind}")),
    }
}

fn decode_hex(input: &str) -> Result<String, String> {
    let bytes = decode_hex_bytes(input)?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

fn parse_hex_path_list(input: &str) -> Result<Vec<String>, String> {
    if input.is_empty() {
        return Ok(Vec::new());
    }

    input
        .split('\u{1f}')
        .filter(|value| !value.is_empty())
        .map(decode_hex)
        .collect()
}

fn parse_string_map_field(
    fields: &BTreeMap<String, String>,
    key: &str,
) -> Result<BTreeMap<String, String>, String> {
    let Some(encoded) = fields.get(key) else {
        return Ok(BTreeMap::new());
    };

    if encoded.is_empty() {
        return Ok(BTreeMap::new());
    }

    encoded
        .split('\u{1e}')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let mut segments = entry.split('\u{1f}');
            let map_key = segments
                .next()
                .ok_or_else(|| format!("missing map key for field: {key}"))
                .and_then(decode_hex)?;
            let map_value = segments
                .next()
                .ok_or_else(|| format!("missing map value for field: {key}"))
                .and_then(decode_hex)?;
            Ok((map_key, map_value))
        })
        .collect()
}

fn decode_hex_bytes(input: &str) -> Result<Vec<u8>, String> {
    if input.len() % 2 != 0 {
        return Err("hex payload must have an even length".into());
    }

    let mut bytes = Vec::with_capacity(input.len() / 2);
    let mut chars = input.chars();

    while let (Some(high), Some(low)) = (chars.next(), chars.next()) {
        bytes.push((hex_char_to_u8(high)? << 4) | hex_char_to_u8(low)?);
    }

    Ok(bytes)
}

fn hex_char_to_u8(input: char) -> Result<u8, String> {
    match input {
        '0'..='9' => Ok((input as u8) - b'0'),
        'a'..='f' => Ok((input as u8) - b'a' + 10),
        'A'..='F' => Ok((input as u8) - b'A' + 10),
        _ => Err(format!("invalid hex character: {input}")),
    }
}

fn set_last_result(result: String) {
    LAST_RESULT.with(|buffer| {
        *buffer.borrow_mut() = result.into_bytes();
    });
}

fn write_error(error: String) -> u32 {
    set_last_result(render_error_json(&error));
    0
}

fn render_boot_summary_json(summary: &crate::protocol::HostBootstrapSummary) -> String {
    format!(
        "{{\"engineName\":\"{}\",\"supportsInterrupts\":{},\"supportsModuleLoader\":{},\"workspaceRoot\":\"{}\"}}",
        escape_json(&summary.engine_name),
        summary.supports_interrupts,
        summary.supports_module_loader,
        escape_json(&summary.workspace_root),
    )
}

fn render_session_handle_json(snapshot: &crate::protocol::SessionSnapshot) -> String {
    let package_name = snapshot
        .package_name
        .as_ref()
        .map(|name| format!("\"{}\"", escape_json(name)))
        .unwrap_or_else(|| "null".into());

    format!(
        "{{\"sessionId\":\"{}\",\"workspaceRoot\":\"{}\",\"packageName\":{},\"fileCount\":{}}}",
        escape_json(&snapshot.session_id),
        escape_json(&snapshot.workspace_root),
        package_name,
        snapshot.archive.file_count,
    )
}

fn render_run_plan_json(plan: &crate::protocol::RunPlan) -> String {
    let command_kind = match plan.command_kind {
        crate::protocol::RunCommandKind::NpmScript => "npm-script",
        crate::protocol::RunCommandKind::NodeEntrypoint => "node-entrypoint",
    };
    let resolved_script = plan
        .resolved_script
        .as_ref()
        .map(|value| format!("\"{}\"", escape_json(value)))
        .unwrap_or_else(|| "null".into());

    format!(
        "{{\"cwd\":\"{}\",\"entrypoint\":\"{}\",\"commandLine\":\"{}\",\"envCount\":{},\"commandKind\":\"{}\",\"resolvedScript\":{}}}",
        escape_json(&plan.cwd),
        escape_json(&plan.entrypoint),
        escape_json(&plan.command_line),
        plan.env_count,
        command_kind,
        resolved_script,
    )
}

fn render_process_info_json(process_info: &HostProcessInfo) -> String {
    let command_kind = match process_info.command_kind {
        crate::protocol::RunCommandKind::NpmScript => "npm-script",
        crate::protocol::RunCommandKind::NodeEntrypoint => "node-entrypoint",
    };
    let env = process_info
        .env
        .iter()
        .map(|(key, value)| format!("\"{}\":\"{}\"", escape_json(key), escape_json(value),))
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "{{\"cwd\":\"{}\",\"argv\":{},\"env\":{{{}}},\"execPath\":\"{}\",\"platform\":\"{}\",\"entrypoint\":\"{}\",\"commandLine\":\"{}\",\"commandKind\":\"{}\"}}",
        escape_json(&process_info.cwd),
        render_string_array_json(&process_info.argv),
        env,
        escape_json(&process_info.exec_path),
        escape_json(&process_info.platform),
        escape_json(&process_info.entrypoint),
        escape_json(&process_info.command_line),
        command_kind,
    )
}

fn render_runtime_context_json(context: &HostRuntimeContext) -> String {
    format!(
        "{{\"contextId\":\"{}\",\"sessionId\":\"{}\",\"process\":{}}}",
        escape_json(&context.context_id),
        escape_json(&context.session_id),
        render_process_info_json(&context.process),
    )
}

fn render_runtime_response_json(response: &HostRuntimeResponse) -> String {
    match response {
        HostRuntimeResponse::Bindings(bindings) => format!(
            "{{\"kind\":\"runtime-bindings\",\"bindings\":{}}}",
            render_runtime_bindings_json(bindings)
        ),
        HostRuntimeResponse::TimerScheduled { timer } => format!(
            "{{\"kind\":\"timer-scheduled\",\"timer\":{}}}",
            render_runtime_timer_json(timer)
        ),
        HostRuntimeResponse::TimerCleared { timer_id, existed } => format!(
            "{{\"kind\":\"timer-cleared\",\"timerId\":\"{}\",\"existed\":{}}}",
            escape_json(timer_id),
            existed
        ),
        HostRuntimeResponse::TimerList { now_ms, timers } => format!(
            "{{\"kind\":\"timer-list\",\"nowMs\":{},\"timers\":{}}}",
            now_ms,
            render_runtime_timer_array_json(timers)
        ),
        HostRuntimeResponse::TimerFired { now_ms, timers } => format!(
            "{{\"kind\":\"timer-fired\",\"nowMs\":{},\"timers\":{}}}",
            now_ms,
            render_runtime_timer_array_json(timers)
        ),
        HostRuntimeResponse::ProcessInfo(process) => format!(
            "{{\"kind\":\"process-info\",\"process\":{}}}",
            render_process_info_json(process)
        ),
        HostRuntimeResponse::ProcessCwd { cwd } => format!(
            "{{\"kind\":\"process-cwd\",\"cwd\":\"{}\"}}",
            escape_json(cwd)
        ),
        HostRuntimeResponse::ProcessArgv { argv } => format!(
            "{{\"kind\":\"process-argv\",\"argv\":{}}}",
            render_string_array_json(argv)
        ),
        HostRuntimeResponse::ProcessEnv { env } => {
            let entries = env
                .iter()
                .map(|(key, value)| format!("\"{}\":\"{}\"", escape_json(key), escape_json(value)))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{\"kind\":\"process-env\",\"env\":{{{entries}}}}}")
        }
        HostRuntimeResponse::PathValue { value } => format!(
            "{{\"kind\":\"path-value\",\"value\":\"{}\"}}",
            escape_json(value)
        ),
        HostRuntimeResponse::Fs(response) => format!(
            "{{\"kind\":\"fs\",\"response\":{}}}",
            render_fs_response_json(response)
        ),
    }
}

fn render_runtime_bindings_json(bindings: &HostRuntimeBindings) -> String {
    let globals = render_string_array_json(&bindings.globals);
    let builtins = bindings
        .builtins
        .iter()
        .map(render_runtime_builtin_json)
        .collect::<Vec<_>>()
        .join(",");

    format!(
        "{{\"contextId\":\"{}\",\"engineName\":\"{}\",\"entrypoint\":\"{}\",\"globals\":{},\"builtins\":[{}]}}",
        escape_json(&bindings.context_id),
        escape_json(&bindings.engine_name),
        escape_json(&bindings.entrypoint),
        globals,
        builtins,
    )
}

fn render_runtime_builtin_json(builtin: &HostRuntimeBuiltinSpec) -> String {
    format!(
        "{{\"name\":\"{}\",\"globals\":{},\"modules\":{},\"commandPrefixes\":{}}}",
        escape_json(&builtin.name),
        render_string_array_json(&builtin.globals),
        render_string_array_json(&builtin.modules),
        render_string_array_json(&builtin.command_prefixes),
    )
}

fn render_runtime_timer_json(timer: &HostRuntimeTimer) -> String {
    let kind = match timer.kind {
        HostRuntimeTimerKind::Timeout => "timeout",
        HostRuntimeTimerKind::Interval => "interval",
    };

    format!(
        "{{\"timerId\":\"{}\",\"kind\":\"{}\",\"delayMs\":{},\"dueAtMs\":{}}}",
        escape_json(&timer.timer_id),
        kind,
        timer.delay_ms,
        timer.due_at_ms,
    )
}

fn render_runtime_timer_array_json(timers: &[HostRuntimeTimer]) -> String {
    let items = timers
        .iter()
        .map(render_runtime_timer_json)
        .collect::<Vec<_>>()
        .join(",");

    format!("[{items}]")
}

fn render_error_json(message: &str) -> String {
    format!("{{\"error\":\"{}\"}}", escape_json(message))
}

fn render_workspace_file_summaries_json(files: &[crate::protocol::WorkspaceFileSummary]) -> String {
    let items = files
        .iter()
        .map(|file| {
            format!(
                "{{\"path\":\"{}\",\"size\":{},\"isText\":{}}}",
                escape_json(&file.path),
                file.size,
                file.is_text,
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    format!("[{items}]")
}

fn render_workspace_entry_json(entry: &crate::protocol::WorkspaceEntrySummary) -> String {
    let kind = match entry.kind {
        crate::protocol::WorkspaceEntryKind::File => "file",
        crate::protocol::WorkspaceEntryKind::Directory => "directory",
    };

    format!(
        "{{\"path\":\"{}\",\"kind\":\"{}\",\"size\":{},\"isText\":{}}}",
        escape_json(&entry.path),
        kind,
        entry.size,
        entry.is_text,
    )
}

fn render_workspace_entries_json(entries: &[crate::protocol::WorkspaceEntrySummary]) -> String {
    let items = entries
        .iter()
        .map(render_workspace_entry_json)
        .collect::<Vec<_>>()
        .join(",");

    format!("[{items}]")
}

fn render_workspace_file_json(file: &VirtualFile) -> String {
    let text_content = if file.is_text {
        format!("\"{}\"", escape_json(&String::from_utf8_lossy(&file.bytes)))
    } else {
        "null".into()
    };

    format!(
        "{{\"path\":\"{}\",\"size\":{},\"isText\":{},\"textContent\":{},\"bytesHex\":\"{}\"}}",
        escape_json(&file.path),
        file.bytes.len(),
        file.is_text,
        text_content,
        encode_hex(&file.bytes),
    )
}

fn render_workspace_files_json(files: &[VirtualFile]) -> String {
    let items = files
        .iter()
        .map(render_workspace_file_json)
        .collect::<Vec<_>>()
        .join(",");

    format!("[{items}]")
}

fn render_fs_response_json(response: &HostFsResponse) -> String {
    match response {
        HostFsResponse::Exists { path, exists } => format!(
            "{{\"kind\":\"exists\",\"path\":\"{}\",\"exists\":{}}}",
            escape_json(path),
            exists,
        ),
        HostFsResponse::Entry(entry) => format!(
            "{{\"kind\":\"entry\",\"entry\":{}}}",
            render_workspace_entry_json(entry)
        ),
        HostFsResponse::DirectoryEntries(entries) => format!(
            "{{\"kind\":\"directory-entries\",\"entries\":{}}}",
            render_workspace_entries_json(entries)
        ),
        HostFsResponse::File {
            path,
            size,
            is_text,
            text_content,
            bytes,
        } => {
            let text_content = text_content
                .as_ref()
                .map(|value| format!("\"{}\"", escape_json(value)))
                .unwrap_or_else(|| "null".into());

            format!(
                "{{\"kind\":\"file\",\"path\":\"{}\",\"size\":{},\"isText\":{},\"textContent\":{},\"bytesHex\":\"{}\"}}",
                escape_json(path),
                size,
                is_text,
                text_content,
                encode_hex(bytes),
            )
        }
    }
}

fn render_string_array_json(values: &[String]) -> String {
    let items = values
        .iter()
        .map(|value| format!("\"{}\"", escape_json(value)))
        .collect::<Vec<_>>()
        .join(",");

    format!("[{items}]")
}

fn render_preview_request_hint_json(hint: &crate::protocol::PreviewRequestHint) -> String {
    let kind = match hint.kind {
        crate::protocol::PreviewRequestKind::RootDocument => "root-document",
        crate::protocol::PreviewRequestKind::RootEntry => "root-entry",
        crate::protocol::PreviewRequestKind::FallbackRoot => "fallback-root",
        crate::protocol::PreviewRequestKind::RuntimeState => "runtime-state",
        crate::protocol::PreviewRequestKind::WorkspaceState => "workspace-state",
        crate::protocol::PreviewRequestKind::FileIndex => "file-index",
        crate::protocol::PreviewRequestKind::DiagnosticsState => "diagnostics-state",
        crate::protocol::PreviewRequestKind::RuntimeStylesheet => "runtime-stylesheet",
        crate::protocol::PreviewRequestKind::WorkspaceFile => "workspace-file",
        crate::protocol::PreviewRequestKind::WorkspaceAsset => "workspace-asset",
        crate::protocol::PreviewRequestKind::NotFound => "not-found",
    };
    let workspace_path = hint
        .workspace_path
        .as_ref()
        .map(|value| format!("\"{}\"", escape_json(value)))
        .unwrap_or_else(|| "null".into());
    let document_root = hint
        .document_root
        .as_ref()
        .map(|value| format!("\"{}\"", escape_json(value)))
        .unwrap_or_else(|| "null".into());
    let hydrate_paths = render_string_array_json(&hint.hydrate_paths);

    format!(
        "{{\"kind\":\"{kind}\",\"workspacePath\":{workspace_path},\"documentRoot\":{document_root},\"hydratePaths\":{hydrate_paths}}}"
    )
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut result = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        result.push(hex_nibble(byte >> 4));
        result.push(hex_nibble(byte & 0x0f));
    }

    result
}

fn hex_nibble(value: u8) -> char {
    match value {
        0..=9 => (b'0' + value) as char,
        10..=15 => (b'a' + (value - 10)) as char,
        _ => '0',
    }
}

fn escape_json(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
