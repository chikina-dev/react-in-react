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
    ArchiveStats, CapabilityMatrix, HostBootstrapSummary, PreviewAssetHint, PreviewRootHint,
    PreviewRootKind, RunPlan, RunRequest, SessionSnapshot, SessionState, WorkspaceFileSummary,
};
pub use vfs::{VirtualFile, VirtualFileSystem, normalize_posix_path};

#[cfg(test)]
mod tests {
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
            host.resolve_preview_hydration_paths(&session.session_id, "/")
                .expect("preview hydration paths should resolve"),
            vec![
                "/workspace/package.json".to_string(),
                "/workspace/src/main.tsx".to_string(),
            ]
        );
        assert_eq!(
            host.resolve_preview_root_hint(&session.session_id)
                .expect("preview root hint should resolve")
                .path
                .as_deref(),
            Some("/workspace/src/main.tsx")
        );
        assert_eq!(
            host.resolve_preview_asset_hint(&session.session_id, "/src/main.tsx")
                .expect("preview asset hint should resolve")
                .workspace_path
                .as_deref(),
            Some("/workspace/src/main.tsx")
        );
    }
}
