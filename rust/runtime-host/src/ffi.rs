use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::engine::NullEngineAdapter;
use crate::host::RuntimeHostCore;
use crate::protocol::{ArchiveStats, RunRequest};
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
        let result =
            host.borrow_mut()
                .create_session_with_id(session_id, archive, package_name, files);

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
pub extern "C" fn runtime_host_resolve_preview_hydration_paths_json(
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
            .resolve_preview_hydration_paths(&session_id, &relative_path);

        match result {
            Ok(paths) => set_last_result(render_string_array_json(&paths)),
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
    format!(
        "{{\"cwd\":\"{}\",\"entrypoint\":\"{}\",\"commandLine\":\"{}\",\"envCount\":{}}}",
        escape_json(&plan.cwd),
        escape_json(&plan.entrypoint),
        escape_json(&plan.command_line),
        plan.env_count,
    )
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

fn render_string_array_json(values: &[String]) -> String {
    let items = values
        .iter()
        .map(|value| format!("\"{}\"", escape_json(value)))
        .collect::<Vec<_>>()
        .join(",");

    format!("[{items}]")
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
