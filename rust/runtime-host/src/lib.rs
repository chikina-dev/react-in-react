pub mod engine;
pub mod error;
pub mod ffi;
pub mod host;
pub mod protocol;
pub mod vfs;

pub use engine::{EngineAdapter, EngineDescriptor, NullEngineAdapter};
pub use error::{RuntimeHostError, RuntimeHostResult};
pub use host::RuntimeHostCore;
pub use protocol::{
    ArchiveStats, CapabilityMatrix, HostBootstrapSummary, HostContextFsCommand, HostFsCommand,
    HostFsResponse, HostProcessInfo, HostRuntimeCommand, HostRuntimeContext, HostRuntimeResponse,
    PreviewRequestHint, PreviewRequestKind, RunPlan, RunRequest, SessionSnapshot, SessionState,
    WorkspaceEntryKind, WorkspaceEntrySummary, WorkspaceFileSummary,
};
pub use vfs::{VirtualFile, VirtualFileSystem, normalize_posix_path};

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn host_boot_summary_uses_engine_descriptor() {
        let host = RuntimeHostCore::new(NullEngineAdapter::default());
        let summary = host.boot_summary();

        assert_eq!(summary.engine_name, "null-engine");
        assert!(summary.supports_interrupts);
        assert!(summary.supports_module_loader);
    }

    #[test]
    fn create_session_mounts_workspace_files() {
        let mut host = RuntimeHostCore::new(NullEngineAdapter::default());
        let archive = ArchiveStats {
            file_name: "demo.zip".into(),
            file_count: 2,
            directory_count: 1,
            root_prefix: Some("demo".into()),
        };

        let session = host
            .create_session(
                archive,
                Some("demo-app".into()),
                BTreeMap::from([(String::from("dev"), String::from("vite"))]),
                vec![
                    VirtualFile::text("/workspace/package.json", r#"{"name":"demo-app"}"#),
                    VirtualFile::text("/workspace/src/main.tsx", "export default null;"),
                ],
            )
            .expect("session should be created");

        assert_eq!(session.workspace_root, "/workspace");
        assert_eq!(session.package_name.as_deref(), Some("demo-app"));
        assert!(host.session_file_system(&session.session_id).is_some());
        assert_eq!(
            host.workspace_file_summaries(&session.session_id)
                .expect("workspace files should be listed")
                .len(),
            2
        );
        assert_eq!(
            host.read_workspace_file(&session.session_id, "/workspace/src/main.tsx")
                .expect("workspace file should be readable")
                .bytes,
            b"export default null;"
        );
        assert_eq!(
            host.resolve_preview_request_hint(&session.session_id, "/")
                .expect("preview root request hint should resolve")
                .workspace_path
                .as_deref(),
            Some("/workspace/src/main.tsx")
        );
        assert_eq!(
            host.resolve_preview_request_hint(&session.session_id, "/")
                .expect("preview root request hint should resolve")
                .hydrate_paths,
            vec![
                "/workspace/package.json".to_string(),
                "/workspace/src/main.tsx".to_string(),
            ]
        );
        assert_eq!(
            host.resolve_preview_request_hint(&session.session_id, "/src/main.tsx")
                .expect("preview request hint should resolve")
                .kind,
            PreviewRequestKind::WorkspaceAsset
        );
        assert_eq!(
            host.resolve_preview_request_hint(&session.session_id, "/src/main.tsx")
                .expect("preview request hint should resolve")
                .hydrate_paths,
            vec![
                "/workspace/package.json".to_string(),
                "/workspace/src/main.tsx".to_string(),
            ]
        );
        assert_eq!(
            host.resolve_preview_request_hint(&session.session_id, "/__diagnostics.json")
                .expect("preview diagnostics hint should resolve")
                .kind,
            PreviewRequestKind::DiagnosticsState
        );
        assert_eq!(
            host.stat_workspace_path(&session.session_id, "/workspace/src")
                .expect("workspace directory should be stat-able")
                .kind,
            WorkspaceEntryKind::Directory
        );
        assert_eq!(
            host.read_workspace_directory(&session.session_id, "/workspace")
                .expect("workspace directory entries should resolve")
                .into_iter()
                .map(|entry| entry.path)
                .collect::<Vec<_>>(),
            vec![
                "/workspace/package.json".to_string(),
                "/workspace/src".to_string(),
            ]
        );
        assert_eq!(
            host.create_workspace_directory(&session.session_id, "/workspace/src/generated")
                .expect("workspace directory should be creatable")
                .path,
            "/workspace/src/generated"
        );
        assert_eq!(
            host.write_workspace_file(
                &session.session_id,
                "/workspace/src/generated/app.ts",
                b"export const generated = true;".to_vec(),
                true,
            )
            .expect("workspace file should be writable")
            .size,
            30
        );
        assert_eq!(
            host.read_workspace_file(&session.session_id, "/workspace/src/generated/app.ts")
                .expect("generated file should be readable")
                .bytes,
            b"export const generated = true;"
        );
        assert_eq!(
            host.execute_fs_command(
                &session.session_id,
                HostFsCommand::Exists {
                    cwd: "/workspace".into(),
                    path: "/workspace/src/generated/app.ts".into(),
                },
            )
            .expect("fs exists should resolve"),
            HostFsResponse::Exists {
                path: "/workspace/src/generated/app.ts".into(),
                exists: true,
            }
        );
        assert!(matches!(
            host.execute_fs_command(
                &session.session_id,
                HostFsCommand::ReadDir {
                    cwd: "/workspace".into(),
                    path: "src".into(),
                },
            ),
            Ok(HostFsResponse::DirectoryEntries(entries))
                if entries.iter().any(|entry| entry.path == "/workspace/src/generated")
        ));
        assert_eq!(
            host.build_process_info(
                &session.session_id,
                &RunRequest::new(
                    "src",
                    "node",
                    vec![String::from("main"), String::from("--watch")]
                ),
            )
            .expect("process info should resolve")
            .argv,
            vec![
                "/virtual/node".to_string(),
                "/workspace/src/main.tsx".to_string(),
                "--watch".to_string(),
            ]
        );
        let runtime_context = host
            .create_runtime_context(
                &session.session_id,
                &RunRequest::new("src", "node", vec![String::from("main")]),
            )
            .expect("runtime context should resolve");
        assert!(matches!(
            host.execute_context_fs_command(
                &runtime_context.context_id,
                HostContextFsCommand::ReadFile {
                    path: String::from("main.tsx"),
                },
            ),
            Ok(HostFsResponse::File { path, .. }) if path == "/workspace/src/main.tsx"
        ));
        assert!(matches!(
            host.execute_runtime_command(&runtime_context.context_id, HostRuntimeCommand::ProcessInfo),
            Ok(HostRuntimeResponse::ProcessInfo(process)) if process.cwd == "/workspace/src"
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::ProcessChdir {
                    path: String::from("generated"),
                },
            ),
            Ok(HostRuntimeResponse::ProcessCwd { cwd }) if cwd == "/workspace/src/generated"
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::Fs(HostContextFsCommand::WriteFile {
                    path: String::from("runtime.log"),
                    bytes: b"context write".to_vec(),
                    is_text: true,
                }),
            ),
            Ok(HostRuntimeResponse::Fs(HostFsResponse::Entry(entry)))
                if entry.path == "/workspace/src/generated/runtime.log"
        ));
        assert_eq!(
            host.plan_run(
                &session.session_id,
                &RunRequest::new("src", "node", vec![String::from("main")]),
            )
            .expect("node run plan should resolve")
            .entrypoint,
            "/workspace/src/main.tsx"
        );
        assert!(matches!(
            host.plan_run(
                &session.session_id,
                &RunRequest::new("/workspace/missing", "node", vec![String::from("main")]),
            ),
            Err(RuntimeHostError::DirectoryNotFound(path)) if path == "/workspace/missing"
        ));
        assert!(matches!(
            host.plan_run(
                &session.session_id,
                &RunRequest::new("/workspace/package.json", "node", vec![String::from("main")]),
            ),
            Err(RuntimeHostError::NotADirectory(path)) if path == "/workspace/package.json"
        ));
        assert!(matches!(
            host.plan_run(
                &session.session_id,
                &RunRequest::new("/tmp", "node", vec![String::from("main")]),
            ),
            Err(RuntimeHostError::InvalidWorkingDirectory(path)) if path == "/tmp"
        ));
        assert!(matches!(
            host.plan_run(
                &session.session_id,
                &RunRequest::new("/workspace", "node", vec![String::from("missing")]),
            ),
            Err(RuntimeHostError::EntrypointNotFound(path)) if path == "/workspace/missing"
        ));
    }
}
