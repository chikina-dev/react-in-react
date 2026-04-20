pub mod engine;
pub mod error;
pub mod ffi;
pub mod host;
pub mod protocol;
pub mod vfs;

pub use engine::{
    EngineAdapter, EngineContextHandle, EngineContextSnapshot, EngineContextState,
    EngineDescriptor, EngineEvalMode, EngineEvalOutcome, EngineJobDrain,
    EngineRegisteredModule, EngineSessionHandle, NullEngineAdapter, QuickJsNgEngineAdapter,
};
pub use error::{RuntimeHostError, RuntimeHostResult};
pub use host::RuntimeHostCore;
pub use protocol::{
    ArchiveStats, CapabilityMatrix, HostBootstrapSummary, HostContextFsCommand, HostFsCommand,
    HostFsResponse, HostProcessInfo, HostRuntimeBindings, HostRuntimeBootstrapModule,
    HostRuntimeBootstrapPlan, HostRuntimeBuiltinSpec, HostRuntimeCommand, HostRuntimeConsoleLevel,
    HostRuntimeContext, HostRuntimeEngineBoot, HostRuntimeEvent, HostRuntimeHttpRequest,
    HostRuntimeHttpServer, HostRuntimeHttpServerKind, HostRuntimeModuleRecord,
    HostRuntimeModuleSource, HostRuntimePort, HostRuntimePortProtocol, HostRuntimeResponse,
    HostRuntimeStdioStream, HostRuntimeTimer, HostRuntimeTimerKind,
    PreviewRequestHint, PreviewRequestKind, PreviewResponseDescriptor, PreviewResponseKind,
    RunPlan, RunRequest, SessionSnapshot, SessionState, WorkspaceEntryKind, WorkspaceEntrySummary,
    WorkspaceFileSummary,
};
pub use vfs::{VirtualFile, VirtualFileSystem, normalize_posix_path};

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::protocol::HostRuntimeModuleFormat;

    #[test]
    fn host_boot_summary_uses_engine_descriptor() {
        let host = RuntimeHostCore::new(NullEngineAdapter::default());
        let summary = host.boot_summary();

        assert_eq!(summary.engine_name, "null-engine");
        assert!(summary.supports_interrupts);
        assert!(summary.supports_module_loader);
    }

    #[test]
    fn runtime_context_boots_engine_context_lifecycle() {
        let mut host = RuntimeHostCore::new(NullEngineAdapter::default());
        let session = host
            .create_session(
                ArchiveStats {
                    file_name: "engine.zip".into(),
                    file_count: 1,
                    directory_count: 1,
                    root_prefix: None,
                },
                Some("engine-app".into()),
                BTreeMap::new(),
                vec![VirtualFile::text(
                    "/workspace/src/main.js",
                    "console.log('hello from engine');",
                )],
            )
            .expect("session should be created");
        let runtime_context = host
            .create_runtime_context(
                &session.session_id,
                &RunRequest::new("/workspace/src", "node", vec![String::from("main")]),
            )
            .expect("runtime context should be created");

        assert_eq!(
            host.describe_engine_context(&runtime_context.context_id)
                .expect("engine context should be described")
                .state,
            EngineContextState::Booted
        );
        assert_eq!(
            host.describe_engine_context(&runtime_context.context_id)
                .expect("engine context should be described")
                .entrypoint,
            "/workspace/src/main.js"
        );
        assert_eq!(
            host.describe_engine_context(&runtime_context.context_id)
                .expect("engine context should be described")
                .registered_modules,
            0
        );

        let eval = host
            .eval_engine_context(
                &runtime_context.context_id,
                "/workspace/src/main.js",
                "console.log('hello from engine');",
                false,
            )
            .expect("null engine eval should succeed");
        assert_eq!(eval.state, EngineContextState::Ready);
        assert!(eval.result_summary.contains("null-engine skipped"));

        assert_eq!(
            host.drain_engine_jobs(&runtime_context.context_id)
                .expect("engine jobs should drain"),
            EngineJobDrain {
                drained_jobs: 0,
                pending_jobs: 0,
            }
        );
        host.interrupt_engine_context(&runtime_context.context_id, "test interrupt")
            .expect("engine interrupt should succeed");
        host.drop_runtime_context(&runtime_context.context_id)
            .expect("runtime context should be dropped");
        assert!(matches!(
            host.describe_engine_context(&runtime_context.context_id),
            Err(RuntimeHostError::RuntimeContextNotFound(_))
        ));
    }

    #[test]
    fn quickjs_ng_engine_scaffold_reports_unlinked_vm() {
        let mut host = RuntimeHostCore::new(QuickJsNgEngineAdapter::default());
        let session = host
            .create_session(
                ArchiveStats {
                    file_name: "quickjs.zip".into(),
                    file_count: 1,
                    directory_count: 1,
                    root_prefix: None,
                },
                Some("quickjs-app".into()),
                BTreeMap::new(),
                vec![VirtualFile::text(
                    "/workspace/src/main.js",
                    "console.log('quickjs');",
                )],
            )
            .expect("session should be created");
        let runtime_context = host
            .create_runtime_context(
                &session.session_id,
                &RunRequest::new("/workspace/src", "node", vec![String::from("main")]),
            )
            .expect("runtime context should be created");

        assert_eq!(host.boot_summary().engine_name, "quickjs-ng-stub");
        let snapshot = host
            .describe_engine_context(&runtime_context.context_id)
            .expect("engine context should exist");
        assert_eq!(snapshot.state, EngineContextState::Booted);
        assert_eq!(snapshot.registered_modules, 0);
        assert!(snapshot.module_loader_roots.is_empty());
        assert!(matches!(
            host.execute_runtime_command(&runtime_context.context_id, HostRuntimeCommand::BootEngine),
            Err(RuntimeHostError::EngineFailure(message))
                if message.contains("quickjs-ng adapter scaffold registered")
        ));
        let snapshot = host
            .describe_engine_context(&runtime_context.context_id)
            .expect("engine context should exist");
        assert_eq!(snapshot.registered_modules, 7);
        assert_eq!(
            snapshot.bootstrap_specifier.as_deref(),
            Some("runtime:bootstrap")
        );
        assert_eq!(
            snapshot.module_loader_roots,
            vec![
                String::from("/workspace/node_modules"),
                String::from("/workspace/src/node_modules"),
            ]
        );
        assert_eq!(
            host.list_engine_modules(&runtime_context.context_id)
                .expect("engine modules should be listed")
                .len(),
            7
        );
        assert_eq!(
            host.read_engine_module(&runtime_context.context_id, "runtime:bootstrap")
                .expect("bootstrap module should resolve")
                .specifier,
            "runtime:bootstrap"
        );
        assert_eq!(
            host.resolve_runtime_module(&runtime_context.context_id, None, "node:process")
                .expect("registered module should resolve")
                .resolved_specifier,
            "node:process"
        );
        let loader_plan = host
            .describe_runtime_module_loader(&runtime_context.context_id)
            .expect("module loader plan should resolve");
        assert_eq!(loader_plan.context_id, runtime_context.context_id);
        assert_eq!(loader_plan.cwd, "/workspace/src");
        assert_eq!(loader_plan.workspace_root, "/workspace");
        assert_eq!(loader_plan.entry_module.resolved_specifier, "/workspace/src/main.js");
        assert_eq!(loader_plan.entry_module.format, HostRuntimeModuleFormat::Module);
        assert!(loader_plan
            .registered_specifiers
            .contains(&String::from("runtime:bootstrap")));
        assert_eq!(
            loader_plan.node_module_search_roots,
            vec![
                String::from("/workspace/node_modules"),
                String::from("/workspace/src/node_modules"),
            ]
        );
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
        let runtime_context = host
            .create_runtime_context(
                &session.session_id,
                &RunRequest::new("/workspace/src", "node", vec![String::from("main.tsx")]),
            )
            .expect("runtime context should be created");
        let relative_module = host
            .resolve_runtime_module(
                &runtime_context.context_id,
                Some("/workspace/src/main.tsx"),
                "./generated/app",
            )
            .expect("relative module should resolve");
        assert_eq!(relative_module.resolved_specifier, "/workspace/src/generated/app.ts");
        let relative_source = host
            .load_runtime_module(&runtime_context.context_id, &relative_module.resolved_specifier)
            .expect("relative module should load");
        assert!(relative_source.source.contains("generated"));
        let loader_plan = host
            .describe_runtime_module_loader(&runtime_context.context_id)
            .expect("module loader plan should resolve");
        assert_eq!(
            loader_plan.entry_module.resolved_specifier,
            "/workspace/src/main.tsx"
        );
        assert_eq!(
            loader_plan.node_module_search_roots,
            vec![
                String::from("/workspace/node_modules"),
                String::from("/workspace/src/node_modules"),
            ]
        );
        assert_eq!(
            host.write_workspace_file(
                &session.session_id,
                "/workspace/package.json",
                br#"{"name":"renamed-app","scripts":{"preview":"vite preview"},"dependencies":{"react":"^19.0.0"},"devDependencies":{"vite":"^8.0.0"}}"#.to_vec(),
                true,
            )
            .expect("package manifest should be writable")
            .path,
            "/workspace/package.json"
        );
        assert_eq!(
            host.plan_run(
                &session.session_id,
                &RunRequest::new(
                    "/workspace",
                    "npm",
                    vec![String::from("run"), String::from("preview")],
                ),
            )
            .expect("updated package scripts should be used")
            .resolved_script,
            Some(String::from("vite preview"))
        );
        assert_eq!(
            host.session_snapshot(&session.session_id)
                .expect("session snapshot should exist")
                .package_name
                .as_deref(),
            Some("renamed-app")
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
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::DescribeBindings,
            ),
            Ok(HostRuntimeResponse::Bindings(bindings))
                if bindings.globals.contains(&"process".to_string())
                    && bindings
                        .builtins
                        .iter()
                        .any(|builtin| builtin.name == "fs"
                            && builtin.modules.contains(&"node:fs".to_string()))
                    && bindings
                        .builtins
                        .iter()
                        .any(|builtin| builtin.name == "console"
                            && builtin.command_prefixes == vec!["console".to_string()])
                    && bindings
                        .builtins
                        .iter()
                        .any(|builtin| builtin.name == "timers"
                            && builtin.command_prefixes == vec!["timers".to_string()])
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::DescribeBootstrap,
            ),
            Ok(HostRuntimeResponse::BootstrapPlan(plan))
                if plan.bootstrap_specifier == "runtime:bootstrap"
                    && plan.entrypoint == "/workspace/src/main.tsx"
                    && plan
                        .modules
                        .iter()
                        .any(|module| module.specifier == "node:process"
                            && module.source.contains("process.cwd"))
                    && plan
                        .modules
                        .iter()
                        .any(|module| module.specifier == "runtime:bootstrap"
                            && module.source.contains("import(\"/workspace/src/main.tsx\")"))
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::StdioWrite {
                    stream: HostRuntimeStdioStream::Stdout,
                    chunk: String::from("hello stdout"),
                },
            ),
            Ok(HostRuntimeResponse::EventQueued { queue_len }) if queue_len == 1
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::ConsoleEmit {
                    level: HostRuntimeConsoleLevel::Warn,
                    values: vec![String::from("watch"), String::from("out")],
                },
            ),
            Ok(HostRuntimeResponse::EventQueued { queue_len }) if queue_len == 3
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::DrainEvents,
            ),
            Ok(HostRuntimeResponse::RuntimeEvents { events })
                if events
                    == vec![
                        HostRuntimeEvent::Stdout {
                            chunk: String::from("hello stdout"),
                        },
                        HostRuntimeEvent::Console {
                            level: HostRuntimeConsoleLevel::Warn,
                            line: String::from("watch out"),
                        },
                        HostRuntimeEvent::Stderr {
                            chunk: String::from("watch out"),
                        },
                    ]
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::PortListen {
                    port: None,
                    protocol: HostRuntimePortProtocol::Http,
                },
            ),
            Ok(HostRuntimeResponse::PortListening { port })
                if port.port == 3000 && port.protocol == HostRuntimePortProtocol::Http
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::PortListen {
                    port: Some(4100),
                    protocol: HostRuntimePortProtocol::Http,
                },
            ),
            Ok(HostRuntimeResponse::PortListening { port })
                if port.port == 4100 && port.protocol == HostRuntimePortProtocol::Http
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::PortList,
            ),
            Ok(HostRuntimeResponse::PortList { ports })
                if ports
                    == vec![
                        HostRuntimePort {
                            port: 3000,
                            protocol: HostRuntimePortProtocol::Http,
                        },
                        HostRuntimePort {
                            port: 4100,
                            protocol: HostRuntimePortProtocol::Http,
                        },
                    ]
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::HttpServePreview { port: Some(4200) },
            ),
            Ok(HostRuntimeResponse::HttpServerListening { server })
                if server
                    == HostRuntimeHttpServer {
                        port: HostRuntimePort {
                            port: 4200,
                            protocol: HostRuntimePortProtocol::Http,
                        },
                        kind: HostRuntimeHttpServerKind::Preview,
                        cwd: String::from("/workspace/src"),
                        entrypoint: String::from("/workspace/src/main.tsx"),
                    }
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::HttpListServers,
            ),
            Ok(HostRuntimeResponse::HttpServerList { servers })
                if servers
                    == vec![HostRuntimeHttpServer {
                        port: HostRuntimePort {
                            port: 4200,
                            protocol: HostRuntimePortProtocol::Http,
                        },
                        kind: HostRuntimeHttpServerKind::Preview,
                        cwd: String::from("/workspace/src"),
                        entrypoint: String::from("/workspace/src/main.tsx"),
                    }]
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::HttpResolvePreview {
                    request: crate::protocol::HostRuntimeHttpRequest {
                        port: 4200,
                        method: String::from("GET"),
                        relative_path: String::from("/src/main.tsx"),
                        search: String::from("?v=1"),
                    },
                },
            ),
            Ok(HostRuntimeResponse::PreviewRequestResolved {
                server,
                port,
                request,
                request_hint,
                response_descriptor,
            }) if server
                == HostRuntimeHttpServer {
                port: HostRuntimePort {
                    port: 4200,
                    protocol: HostRuntimePortProtocol::Http,
                },
                kind: HostRuntimeHttpServerKind::Preview,
                cwd: String::from("/workspace/src"),
                entrypoint: String::from("/workspace/src/main.tsx"),
            }
                && port
                == HostRuntimePort {
                port: 4200,
                protocol: HostRuntimePortProtocol::Http,
            }
                && request.relative_path == "/src/main.tsx"
                && request.search == "?v=1"
                && request_hint.kind == crate::protocol::PreviewRequestKind::WorkspaceAsset
                && request_hint.workspace_path == Some(String::from("/workspace/src/main.tsx"))
                && request_hint.document_root == Some(String::from("/workspace"))
                && request_hint.hydrate_paths
                    == vec![
                        String::from("/workspace/package.json"),
                        String::from("/workspace/src/main.tsx"),
                    ]
                && response_descriptor
                    == PreviewResponseDescriptor {
                        kind: PreviewResponseKind::WorkspaceAsset,
                        workspace_path: Some(String::from("/workspace/src/main.tsx")),
                        document_root: Some(String::from("/workspace")),
                        hydrate_paths: vec![
                            String::from("/workspace/package.json"),
                            String::from("/workspace/src/main.tsx"),
                        ],
                        status_code: 200,
                        content_type: Some(String::from("text/plain; charset=utf-8")),
                        allow_methods: Vec::new(),
                        omit_body: false,
                    }
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::HttpResolvePreview {
                    request: HostRuntimeHttpRequest {
                        port: 4200,
                        method: String::from("HEAD"),
                        relative_path: String::from("/src/main.tsx"),
                        search: String::new(),
                    },
                },
            ),
            Ok(HostRuntimeResponse::PreviewRequestResolved { response_descriptor, .. })
                if response_descriptor
                    == PreviewResponseDescriptor {
                        kind: PreviewResponseKind::WorkspaceAsset,
                        workspace_path: Some(String::from("/workspace/src/main.tsx")),
                        document_root: Some(String::from("/workspace")),
                        hydrate_paths: Vec::new(),
                        status_code: 200,
                        content_type: Some(String::from("text/plain; charset=utf-8")),
                        allow_methods: Vec::new(),
                        omit_body: true,
                    }
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::HttpResolvePreview {
                    request: HostRuntimeHttpRequest {
                        port: 4200,
                        method: String::from("POST"),
                        relative_path: String::from("/src/main.tsx"),
                        search: String::new(),
                    },
                },
            ),
            Ok(HostRuntimeResponse::PreviewRequestResolved { response_descriptor, .. })
                if response_descriptor
                    == PreviewResponseDescriptor {
                        kind: PreviewResponseKind::MethodNotAllowed,
                        workspace_path: None,
                        document_root: None,
                        hydrate_paths: Vec::new(),
                        status_code: 405,
                        content_type: Some(String::from("application/json; charset=utf-8")),
                        allow_methods: vec![String::from("GET"), String::from("HEAD")],
                        omit_body: false,
                    }
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::HttpCloseServer { port: 4200 },
            ),
            Ok(HostRuntimeResponse::HttpServerClosed { port, existed })
                if port == 4200 && existed
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::DrainEvents,
            ),
            Ok(HostRuntimeResponse::RuntimeEvents { events })
                if events
                    == vec![
                        HostRuntimeEvent::PortListen {
                            port: HostRuntimePort {
                                port: 3000,
                                protocol: HostRuntimePortProtocol::Http,
                            },
                        },
                        HostRuntimeEvent::PortListen {
                            port: HostRuntimePort {
                                port: 4100,
                                protocol: HostRuntimePortProtocol::Http,
                            },
                        },
                        HostRuntimeEvent::PortListen {
                            port: HostRuntimePort {
                                port: 4200,
                                protocol: HostRuntimePortProtocol::Http,
                            },
                        },
                        HostRuntimeEvent::PortClose { port: 4200 },
                    ]
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::PortClose { port: 3000 },
            ),
            Ok(HostRuntimeResponse::PortClosed { port, existed })
                if port == 3000 && existed
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::DrainEvents,
            ),
            Ok(HostRuntimeResponse::RuntimeEvents { events })
                if events == vec![HostRuntimeEvent::PortClose { port: 3000 }]
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::Fs(crate::protocol::HostContextFsCommand::WriteFile {
                    path: String::from("generated/output.json"),
                    bytes: br#"{"ok":true}"#.to_vec(),
                    is_text: true,
                }),
            ),
            Ok(HostRuntimeResponse::Fs(HostFsResponse::Entry(entry)))
                if entry
                    == WorkspaceEntrySummary {
                        path: String::from("/workspace/src/generated/output.json"),
                        kind: WorkspaceEntryKind::File,
                        size: 11,
                        is_text: true,
                    }
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::DrainEvents,
            ),
            Ok(HostRuntimeResponse::RuntimeEvents { events })
                if events
                    == vec![HostRuntimeEvent::WorkspaceChange {
                        entry: WorkspaceEntrySummary {
                            path: String::from("/workspace/src/generated/output.json"),
                            kind: WorkspaceEntryKind::File,
                            size: 11,
                            is_text: true,
                        },
                        revision: 1,
                    }]
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::TimerSchedule {
                    delay_ms: 50,
                    repeat: false,
                },
            ),
            Ok(HostRuntimeResponse::TimerScheduled { timer })
                if timer.timer_id == "runtime-timer-1"
                    && timer.kind == HostRuntimeTimerKind::Timeout
                    && timer.delay_ms == 50
                    && timer.due_at_ms == 50
        ));
        assert!(matches!(
            host.execute_runtime_command(&runtime_context.context_id, HostRuntimeCommand::TimerList,),
            Ok(HostRuntimeResponse::TimerList { now_ms, timers })
                if now_ms == 0 && timers.len() == 1 && timers[0].timer_id == "runtime-timer-1"
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::TimerAdvance { elapsed_ms: 25 },
            ),
            Ok(HostRuntimeResponse::TimerFired { now_ms, timers })
                if now_ms == 25 && timers.is_empty()
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::TimerAdvance { elapsed_ms: 25 },
            ),
            Ok(HostRuntimeResponse::TimerFired { now_ms, timers })
                if now_ms == 50
                    && timers.len() == 1
                    && timers[0].timer_id == "runtime-timer-1"
                    && timers[0].kind == HostRuntimeTimerKind::Timeout
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::TimerSchedule {
                    delay_ms: 10,
                    repeat: true,
                },
            ),
            Ok(HostRuntimeResponse::TimerScheduled { timer })
                if timer.timer_id == "runtime-timer-2"
                    && timer.kind == HostRuntimeTimerKind::Interval
                    && timer.delay_ms == 10
                    && timer.due_at_ms == 60
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::TimerAdvance { elapsed_ms: 35 },
            ),
            Ok(HostRuntimeResponse::TimerFired { now_ms, timers })
                if now_ms == 85
                    && timers.len() == 1
                    && timers[0].timer_id == "runtime-timer-2"
                    && timers[0].kind == HostRuntimeTimerKind::Interval
        ));
        assert!(matches!(
            host.execute_runtime_command(&runtime_context.context_id, HostRuntimeCommand::TimerList,),
            Ok(HostRuntimeResponse::TimerList { now_ms, timers })
                if now_ms == 85
                    && timers.len() == 1
                    && timers[0].timer_id == "runtime-timer-2"
                    && timers[0].due_at_ms == 90
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::TimerClear {
                    timer_id: String::from("runtime-timer-2"),
                },
            ),
            Ok(HostRuntimeResponse::TimerCleared { timer_id, existed })
                if timer_id == "runtime-timer-2" && existed
        ));
        assert!(matches!(
            host.execute_runtime_command(&runtime_context.context_id, HostRuntimeCommand::ProcessInfo),
            Ok(HostRuntimeResponse::ProcessInfo(process)) if process.cwd == "/workspace/src"
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::ProcessStatus,
            ),
            Ok(HostRuntimeResponse::ProcessStatus {
                exited: false,
                exit_code: None,
            })
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
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::PathResolve {
                    segments: vec![String::from("../package.json")],
                },
            ),
            Ok(HostRuntimeResponse::PathValue { value }) if value == "/workspace/src/package.json"
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::PathExtname {
                    path: String::from("/workspace/src/generated/runtime.log"),
                },
            ),
            Ok(HostRuntimeResponse::PathValue { value }) if value == ".log"
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::ProcessExit { code: 0 },
            ),
            Ok(HostRuntimeResponse::ProcessStatus {
                exited: true,
                exit_code: Some(0),
            })
        ));
        assert!(matches!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::DrainEvents,
            ),
            Ok(HostRuntimeResponse::RuntimeEvents { events })
                if events
                    == vec![
                        HostRuntimeEvent::WorkspaceChange {
                            entry: WorkspaceEntrySummary {
                                path: String::from("/workspace/src/generated/runtime.log"),
                                kind: WorkspaceEntryKind::File,
                                size: 13,
                                is_text: true,
                            },
                            revision: 2,
                        },
                        HostRuntimeEvent::ProcessExit { code: 0 },
                    ]
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
