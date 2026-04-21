pub mod engine;
pub mod error;
pub mod ffi;
pub mod host;
pub mod protocol;
pub mod vfs;

pub use engine::{
    EngineAdapter, EngineContextHandle, EngineContextSnapshot, EngineContextState,
    EngineDescriptor, EngineEvalMode, EngineEvalOutcome, EngineJobDrain, EngineRegisteredModule,
    EngineSessionHandle, NullEngineAdapter, QuickJsNgEngineAdapter,
};
pub use error::{RuntimeHostError, RuntimeHostResult};
pub use host::RuntimeHostCore;
pub use protocol::{
    ArchiveStats, CapabilityMatrix, HostBootstrapSummary, HostContextFsCommand, HostFsCommand,
    HostFsResponse, HostProcessInfo, HostRuntimeBindings, HostRuntimeBootstrapModule,
    HostRuntimeBootstrapPlan, HostRuntimeBuiltinSpec, HostRuntimeCommand, HostRuntimeConsoleLevel,
    HostRuntimeContext, HostRuntimeEngineBoot, HostRuntimeEvent, HostRuntimeHttpRequest,
    HostRuntimeHttpServer, HostRuntimeHttpServerKind, HostRuntimeIdleReport,
    HostRuntimeLaunchReport, HostRuntimePreviewLaunchReport, HostRuntimeStartupReport,
    HostRuntimeModuleRecord, HostRuntimeModuleSource, HostRuntimePort, HostRuntimePortProtocol,
    HostRuntimeResponse, HostRuntimeStdioStream, HostRuntimeTimer, HostRuntimeTimerKind,
    PreviewRequestHint, PreviewRequestKind, PreviewResponseDescriptor, PreviewResponseKind,
    RunPlan, RunRequest, SessionSnapshot, SessionState, WorkspaceEntryKind, WorkspaceEntrySummary,
    WorkspaceFileSummary,
};
pub use vfs::{VirtualFile, VirtualFileSystem, normalize_posix_path};

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    #[cfg(not(feature = "quickjs-ng-engine"))]
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
    fn runtime_launch_preview_collapses_boot_and_preview_setup() {
        let mut host = RuntimeHostCore::new(NullEngineAdapter::default());
        let session = host
            .create_session(
                ArchiveStats {
                    file_name: "preview.zip".into(),
                    file_count: 1,
                    directory_count: 1,
                    root_prefix: None,
                },
                Some("preview-app".into()),
                BTreeMap::new(),
                vec![VirtualFile::text(
                    "/workspace/src/main.js",
                    "console.log('hello from preview');",
                )],
            )
            .expect("session should be created");
        let runtime_context = host
            .create_runtime_context(
                &session.session_id,
                &RunRequest::new("/workspace/src", "node", vec![String::from("main")]),
            )
            .expect("runtime context should be created");

        let launched = host
            .execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::LaunchPreview {
                    max_turns: 16,
                    port: Some(3100),
                },
            )
            .expect("launch preview should succeed");

        let report = match launched {
            HostRuntimeResponse::PreviewLaunchReport(report) => report,
            other => panic!("expected preview launch report, got {other:?}"),
        };
        assert!(!report.startup.exited);
        assert_eq!(report.startup.exit_code, None);
        assert_eq!(report.server.as_ref().map(|server| server.port.port), Some(3100));
        assert_eq!(report.port.as_ref().map(|port| port.port), Some(3100));
        assert_eq!(
            report.root_request.as_ref().map(|request| request.relative_path.as_str()),
            Some("/")
        );
        assert_eq!(
            report.root_request_hint.as_ref().map(|hint| &hint.kind),
            Some(&PreviewRequestKind::RootEntry)
        );
        assert_eq!(
            report
                .root_response_descriptor
                .as_ref()
                .map(|descriptor| &descriptor.kind),
            Some(&PreviewResponseKind::AppShell)
        );
    }

    #[test]
    fn launch_runtime_collapses_run_startup_setup() {
        let mut host = RuntimeHostCore::new(NullEngineAdapter::default());
        let session = host
            .create_session(
                ArchiveStats {
                    file_name: "launch.zip".into(),
                    file_count: 2,
                    directory_count: 1,
                    root_prefix: None,
                },
                Some("launch-app".into()),
                BTreeMap::new(),
                vec![
                    VirtualFile::text(
                        "/workspace/package.json",
                        r#"{"name":"launch-app","scripts":{"dev":"node src/server.js"}}"#,
                    ),
                    VirtualFile::text(
                        "/workspace/src/server.js",
                        "console.log('launch runtime');",
                    ),
                ],
            )
            .expect("session should be created");

        let launched = host
            .launch_runtime(
                &session.session_id,
                &RunRequest::new("/workspace/src", "node", vec![String::from("server.js")]),
                16,
                Some(3200),
            )
            .expect("launch runtime should succeed");

        assert_eq!(launched.boot_summary.engine_name, "null-engine");
        assert_eq!(launched.run_plan.command_kind, crate::protocol::RunCommandKind::NodeEntrypoint);
        assert_eq!(launched.run_plan.entrypoint, "/workspace/src/server.js");
        assert_eq!(launched.runtime_context.process.cwd, "/workspace/src");
        assert_eq!(launched.engine_context.state, EngineContextState::Booted);
        assert!(
            launched
                .bindings
                .globals
                .iter()
                .any(|global| global == "process")
        );
        assert!(
            launched
                .bindings
                .globals
                .iter()
                .any(|global| global == "Buffer")
        );
        assert_eq!(launched.bootstrap_plan.bootstrap_specifier, "runtime:bootstrap");
        assert!(launched.startup_logs.iter().any(|line| line.contains("[host] engine=null-engine")));
        assert!(launched.startup_logs.iter().any(|line| line.contains("[context] id=")));
        assert_eq!(
            launched.preview_launch.server.as_ref().map(|server| server.port.port),
            Some(3200)
        );
        assert_eq!(
            launched
                .preview_launch
                .root_request_hint
                .as_ref()
                .map(|hint| &hint.kind),
            Some(&PreviewRequestKind::FallbackRoot)
        );
        assert!(launched.events.iter().any(|event| matches!(
            event,
            HostRuntimeEvent::PortListen { port } if port.port == 3200
        )));
    }

    #[test]
    fn runtime_shutdown_collapses_stop_and_context_drop() {
        let mut host = RuntimeHostCore::new(NullEngineAdapter::default());
        let session = host
            .create_session(
                ArchiveStats {
                    file_name: "shutdown.zip".into(),
                    file_count: 1,
                    directory_count: 1,
                    root_prefix: None,
                },
                Some("shutdown-app".into()),
                BTreeMap::new(),
                vec![VirtualFile::text(
                    "/workspace/src/main.js",
                    "console.log('shutdown');",
                )],
            )
            .expect("session should be created");
        let runtime_context = host
            .create_runtime_context(
                &session.session_id,
                &RunRequest::new("/workspace/src", "node", vec![String::from("main")]),
            )
            .expect("runtime context should be created");

        host.execute_runtime_command(
            &runtime_context.context_id,
            HostRuntimeCommand::HttpServePreview { port: Some(3100) },
        )
        .expect("preview server should start");
        host.execute_runtime_command(
            &runtime_context.context_id,
            HostRuntimeCommand::ProcessExit { code: 0 },
        )
        .expect("process exit should succeed");

        let shutdown = host
            .execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::Shutdown { code: 0 },
            )
            .expect("shutdown should succeed");

        let report = match shutdown {
            HostRuntimeResponse::ShutdownReport(report) => report,
            other => panic!("expected shutdown report, got {other:?}"),
        };
        assert_eq!(report.context_id, runtime_context.context_id);
        assert_eq!(report.session_id, session.session_id);
        assert_eq!(report.exit_code, 0);
        assert_eq!(report.closed_ports.len(), 1);
        assert_eq!(report.closed_servers.len(), 1);
        assert!(report
            .events
            .iter()
            .any(|event| matches!(event, HostRuntimeEvent::ProcessExit { code: 0 })));
        assert!(report
            .events
            .iter()
            .any(|event| matches!(event, HostRuntimeEvent::PortClose { port: 3100 })));
        assert!(matches!(
            host.describe_engine_context(&runtime_context.context_id),
            Err(RuntimeHostError::RuntimeContextNotFound(_))
        ));
    }

    #[cfg(not(feature = "quickjs-ng-engine"))]
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
        assert_eq!(snapshot.registered_modules, 8);
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
            8
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
        assert_eq!(
            loader_plan.entry_module.resolved_specifier,
            "/workspace/src/main.js"
        );
        assert_eq!(
            loader_plan.entry_module.format,
            HostRuntimeModuleFormat::Module
        );
        assert!(
            loader_plan
                .registered_specifiers
                .contains(&String::from("runtime:bootstrap"))
        );
        assert_eq!(
            loader_plan.node_module_search_roots,
            vec![
                String::from("/workspace/node_modules"),
                String::from("/workspace/src/node_modules"),
            ]
        );
    }

    #[cfg(feature = "quickjs-ng-engine")]
    #[test]
    fn quickjs_ng_engine_evaluates_scripts_and_bootstraps_registered_modules() {
        let mut host = RuntimeHostCore::new(QuickJsNgEngineAdapter::default());
        let session = host
            .create_session(
                ArchiveStats {
                    file_name: "quickjs.zip".into(),
                    file_count: 6,
                    directory_count: 3,
                    root_prefix: None,
                },
                Some("quickjs-app".into()),
                BTreeMap::new(),
                vec![
                    VirtualFile::text(
                        "/workspace/src/main.js",
                        r#"import process from "node:process";
import fs from "node:fs";
import path from "node:path";
import { answer as localAnswer } from "./boot.js";
import packageAnswer from "demo-pkg";
import packageFeature from "demo-pkg/feature";
console.log("quickjs native bootstrap");
globalThis.cwdValue = process.cwd();
globalThis.argvCount = process.argv.length;
globalThis.envValue = process.env.RUNTIME_SAMPLE;
globalThis.bootExists = fs.existsSync("./boot.js");
globalThis.bootSource = fs.readFileSync("./boot.js");
globalThis.bootPath = path.resolve("./boot.js");
fs.mkdirSync("../generated", { recursive: true });
fs.writeFileSync("../generated/from-native.txt", "native quickjs");
process.chdir("..");
globalThis.cwdAfterChdir = process.cwd();
globalThis.pendingTimer = setTimeout(() => {
  globalThis.lateTimerHit = 1;
}, 25);
globalThis.intervalCount = 0;
globalThis.intervalId = setInterval(() => {
  globalThis.intervalCount += 1;
  if (globalThis.intervalCount >= 3) {
    clearInterval(globalThis.intervalId);
  }
}, 10);
Promise.resolve().then(() => {
  globalThis.microtaskValue = 1;
  setTimeout(() => {
    globalThis.microtaskTimerHit = 1;
  }, 5);
});
setTimeout(() => {
  globalThis.timerHit = 1;
}, 0);
globalThis.answer = localAnswer + packageAnswer + packageFeature;
globalThis.answer;"#,
                    ),
                    VirtualFile::text("/workspace/src/boot.js", "export const answer = 40 + 2;"),
                    VirtualFile::text(
                        "/workspace/src/later.js",
                        r#"import laterPackage from "late-pkg/feature";
export default laterPackage + 3;"#,
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/demo-pkg/package.json",
                        r#"{"name":"demo-pkg","exports":{".":{"import":"./esm/root.js","default":"./index.cjs"},"./feature":{"default":"./feature.cjs"}}}"#,
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/demo-pkg/esm/root.js",
                        r#"import payload from "../value.json";
export default payload.offset + 1;"#,
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/demo-pkg/index.cjs",
                        r#"const extra = require("./util.cjs");
const payload = require("./value.json");
module.exports = extra + payload.offset;"#,
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/demo-pkg/util.cjs",
                        "module.exports = 1;",
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/demo-pkg/feature.cjs",
                        "module.exports = 2;",
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/demo-pkg/value.json",
                        r#"{"offset":1}"#,
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/late-pkg/package.json",
                        r#"{"name":"late-pkg","exports":{"./feature":{"default":"./index.cjs"}}}"#,
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/late-pkg/index.cjs",
                        r#"const payload = require("./value.json");
module.exports = payload.offset + 4;"#,
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/late-pkg/value.json",
                        r#"{"offset":5}"#,
                    ),
                ],
            )
            .expect("session should be created");
        let mut request = RunRequest::new("/workspace/src", "node", vec![String::from("main")]);
        request
            .env
            .insert(String::from("RUNTIME_SAMPLE"), String::from("present"));
        let runtime_context = host
            .create_runtime_context(&session.session_id, &request)
            .expect("runtime context should be created");

        assert_eq!(
            host.boot_summary().engine_name,
            "quickjs-ng-native-bootstrap-loader"
        );

        let eval = host
            .eval_engine_context(
                &runtime_context.context_id,
                "/workspace/src/main.js",
                "globalThis.quickjsWarmup = 40 + 2; globalThis.quickjsWarmup;",
                false,
            )
            .expect("quickjs-ng should evaluate a simple script");
        assert_eq!(eval.state, EngineContextState::Ready);
        assert!(eval.result_summary.contains("42"));

        let snapshot = host
            .describe_engine_context(&runtime_context.context_id)
            .expect("engine context should exist");
        assert_eq!(snapshot.state, EngineContextState::Ready);

        let startup = host
            .execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::Startup { max_turns: 32 },
            )
            .expect("quickjs-ng should boot runtime and flush it until idle");
        let report = match startup {
            HostRuntimeResponse::StartupReport(report) => report,
            other => panic!("expected runtime startup report, got {other:?}"),
        };
        assert_eq!(report.boot.plan.bootstrap_specifier, "runtime:bootstrap");
        assert_eq!(report.boot.plan.modules.len(), 7);
        assert_eq!(
            report.boot.loader_plan.node_module_search_roots,
            vec![
                String::from("/workspace/node_modules"),
                String::from("/workspace/src/node_modules"),
            ]
        );
        assert!(
            report
                .boot
                .result_summary
                .contains("quickjs-ng booted 7 bootstrap modules")
        );
        assert_eq!(
            report.entry_import_plan.resolved_module.resolved_specifier,
            "/workspace/src/main.js"
        );
        assert!(!report.idle.reached_turn_limit);
        assert_eq!(report.idle.pending_jobs, 0);
        assert_eq!(report.idle.pending_timers, 0);
        assert_eq!(report.idle.now_ms, 30);
        assert_eq!(report.idle.fired_timers, 5);
        assert_eq!(report.idle.turns, 5);
        assert!(!report.exited);
        assert_eq!(report.exit_code, None);

        let imported_answer = host
            .eval_engine_context(
                &runtime_context.context_id,
                "/workspace/answer.js",
                "globalThis.answer",
                false,
            )
            .expect("entry module side effects should run during bootstrap");
        assert!(imported_answer.result_summary.contains("46"));

        let globals = host
            .eval_engine_context(
                &runtime_context.context_id,
                "/workspace/check.js",
                r#"JSON.stringify({
  process: typeof globalThis.process,
  setTimeout: typeof globalThis.setTimeout,
  setInterval: typeof globalThis.setInterval,
  Buffer: typeof globalThis.Buffer,
	})"#,
                false,
            )
            .expect("bootstrap globals should be installed");
        assert!(globals.result_summary.contains(r#""process":"object""#));
        assert!(
            globals
                .result_summary
                .contains(r#""setTimeout":"function""#)
        );
        assert!(
            globals
                .result_summary
                .contains(r#""setInterval":"function""#)
        );
        assert!(globals.result_summary.contains(r#""Buffer":"function""#));

        let builtin_state = host
            .eval_engine_context(
                &runtime_context.context_id,
                "/workspace/builtins.js",
                r#"JSON.stringify({
  cwd: globalThis.cwdValue,
  argvCount: globalThis.argvCount,
  envValue: globalThis.envValue,
  bootExists: globalThis.bootExists,
  bootPath: globalThis.bootPath,
  timerHit: globalThis.timerHit,
  intervalType: typeof globalThis.setInterval,
  cwdAfterChdir: globalThis.cwdAfterChdir,
  bootSourceHasAnswer: globalThis.bootSource.includes("answer = 40 + 2"),
})"#,
                false,
            )
            .expect("native bridge builtins should be usable during bootstrap");
        assert!(
            builtin_state
                .result_summary
                .contains(r#""cwd":"/workspace/src""#)
        );
        assert!(builtin_state.result_summary.contains(r#""argvCount":2"#));
        assert!(
            builtin_state
                .result_summary
                .contains(r#""envValue":"present""#)
        );
        assert!(
            builtin_state
                .result_summary
                .contains(r#""bootExists":true"#)
        );
        assert!(
            builtin_state
                .result_summary
                .contains(r#""bootPath":"/workspace/src/boot.js""#)
        );
        assert!(builtin_state.result_summary.contains(r#""timerHit":1"#));
        assert!(
            builtin_state
                .result_summary
                .contains(r#""intervalType":"function""#)
        );
        assert!(
            builtin_state
                .result_summary
                .contains(r#""cwdAfterChdir":"/workspace""#)
        );
        assert!(
            builtin_state
                .result_summary
                .contains(r#""bootSourceHasAnswer":true"#)
        );

        let generated = host
            .read_workspace_file(&session.session_id, "/workspace/generated/from-native.txt")
            .expect("native fs.writeFileSync should sync back to host workspace");
        assert_eq!(String::from_utf8_lossy(&generated.bytes), "native quickjs");
        assert_eq!(
            host.execute_runtime_command(
                &runtime_context.context_id,
                HostRuntimeCommand::ProcessCwd
            )
            .expect("process cwd should reflect native process.chdir"),
            HostRuntimeResponse::ProcessCwd {
                cwd: String::from("/workspace"),
            }
        );
        let session_snapshot = host
            .session_snapshot(&session.session_id)
            .expect("session snapshot should exist");
        assert_eq!(session_snapshot.revision, 1);
        assert_eq!(session_snapshot.archive.file_count, 13);
        assert_eq!(session_snapshot.archive.directory_count, 7);
        let runtime_state = host
            .eval_engine_context(
                &runtime_context.context_id,
                "/workspace/runtime-state.js",
                r#"JSON.stringify({
  lateTimerHit: globalThis.lateTimerHit,
  intervalCount: globalThis.intervalCount,
  microtaskValue: globalThis.microtaskValue,
  microtaskTimerHit: globalThis.microtaskTimerHit,
})"#,
                false,
            )
            .expect("run-until-idle should flush native microtasks and timers");
        assert!(runtime_state.result_summary.contains(r#""lateTimerHit":1"#));
        assert!(
            runtime_state
                .result_summary
                .contains(r#""intervalCount":3"#)
        );
        assert!(
            runtime_state
                .result_summary
                .contains(r#""microtaskValue":1"#)
        );
        assert!(
            runtime_state
                .result_summary
                .contains(r#""microtaskTimerHit":1"#)
        );
        let timers_after_fire = host
            .execute_runtime_command(&runtime_context.context_id, HostRuntimeCommand::TimerList)
            .expect("idle run should leave no native timers behind");
        let HostRuntimeResponse::TimerList {
            now_ms: after_fire_now_ms,
            timers: remaining_timers,
        } = timers_after_fire
        else {
            panic!("expected timer list after idle run");
        };
        assert_eq!(after_fire_now_ms, 30);
        assert!(remaining_timers.is_empty());
        let runtime_events = host
            .execute_runtime_command(&runtime_context.context_id, HostRuntimeCommand::DrainEvents)
            .expect("workspace change events should be queued after native writes");
        let HostRuntimeResponse::RuntimeEvents { events } = runtime_events else {
            panic!("expected runtime events after native bridge sync");
        };
        assert!(events.iter().any(|event| {
            matches!(
                event,
                HostRuntimeEvent::Console { line, .. } if line == "quickjs native bootstrap"
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                HostRuntimeEvent::Stdout { chunk } if chunk == "quickjs native bootstrap"
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                HostRuntimeEvent::WorkspaceChange { entry, revision }
                    if entry.path == "/workspace/generated" && *revision == 1
            )
        }));
        assert!(events.iter().any(|event| {
            matches!(
                event,
                HostRuntimeEvent::WorkspaceChange { entry, revision }
                    if entry.path == "/workspace/generated/from-native.txt" && *revision == 1
            )
        }));

        host.eval_engine_context(
            &runtime_context.context_id,
            "/workspace/src/later-check.mjs",
            r#"import laterAnswer from "./later.js";
globalThis.lateAnswer = laterAnswer;
export default laterAnswer;"#,
            true,
        )
        .expect("native loader should resolve workspace and node_modules modules on demand");

        let late_state = host
            .eval_engine_context(
                &runtime_context.context_id,
                "/workspace/late-state.js",
                "globalThis.lateAnswer",
                false,
            )
            .expect("late import side effect should persist");
        assert!(late_state.result_summary.contains("12"));

        let snapshot = host
            .describe_engine_context(&runtime_context.context_id)
            .expect("engine context should exist");
        assert_eq!(snapshot.registered_modules, 15);
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
                    VirtualFile::text(
                        "/workspace/package.json",
                        r##"{"name":"demo-app","imports":{"#main":"./src/main.tsx"},"exports":{".":"./src/main.tsx","./self":"./src/generated/app.ts"}}"##,
                    ),
                    VirtualFile::text("/workspace/src/main.tsx", "export default null;"),
                    VirtualFile::text(
                        "/workspace/node_modules/exports-pkg/package.json",
                        r#"{"name":"exports-pkg","exports":{".":{"import":"./esm/root.js","default":"./index.cjs"},"./feature":{"default":"./feature.cjs"}}}"#,
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/exports-pkg/esm/root.js",
                        "export default 'exports-root';",
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/exports-pkg/feature.cjs",
                        "module.exports = 'exports-feature';",
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/browser-pkg/package.json",
                        r#"{"name":"browser-pkg","main":"./server.js","browser":{"./server.js":"./browser.js","./feature.js":"./feature-browser.js"}}"#,
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/browser-pkg/server.js",
                        "export { default } from './feature.js';",
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/browser-pkg/browser.js",
                        "export default 'browser-entry';",
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/browser-pkg/feature.js",
                        "export default 'server-feature';",
                    ),
                    VirtualFile::text(
                        "/workspace/node_modules/browser-pkg/feature-browser.js",
                        "export default 'browser-feature';",
                    ),
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
            10
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
                "/workspace/node_modules/browser-pkg/package.json".to_string(),
                "/workspace/node_modules/exports-pkg/package.json".to_string(),
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
                "/workspace/node_modules/browser-pkg/package.json".to_string(),
                "/workspace/node_modules/exports-pkg/package.json".to_string(),
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
                "/workspace/node_modules".to_string(),
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
        assert_eq!(
            relative_module.resolved_specifier,
            "/workspace/src/generated/app.ts"
        );
        let relative_source = host
            .load_runtime_module(
                &runtime_context.context_id,
                &relative_module.resolved_specifier,
            )
            .expect("relative module should load");
        assert!(relative_source.source.contains("generated"));
        let import_plan = host
            .prepare_runtime_module_import(
                &runtime_context.context_id,
                "./generated/app",
                Some("/workspace/src/main.tsx"),
            )
            .expect("runtime module import plan should resolve");
        assert_eq!(
            import_plan.resolved_module.resolved_specifier,
            "/workspace/src/generated/app.ts"
        );
        assert!(import_plan.loaded_module.source.contains("generated"));
        let package_import = host
            .resolve_runtime_module(
                &runtime_context.context_id,
                Some("/workspace/src/main.tsx"),
                "#main",
            )
            .expect("package imports alias should resolve");
        assert_eq!(package_import.resolved_specifier, "/workspace/src/main.tsx");
        let self_root = host
            .resolve_runtime_module(
                &runtime_context.context_id,
                Some("/workspace/src/main.tsx"),
                "demo-app",
            )
            .expect("package self root should resolve");
        assert_eq!(self_root.resolved_specifier, "/workspace/src/main.tsx");
        let self_subpath = host
            .resolve_runtime_module(
                &runtime_context.context_id,
                Some("/workspace/src/main.tsx"),
                "demo-app/self",
            )
            .expect("package self subpath should resolve");
        assert_eq!(
            self_subpath.resolved_specifier,
            "/workspace/src/generated/app.ts"
        );
        let package_module = host
            .resolve_runtime_module(
                &runtime_context.context_id,
                Some("/workspace/src/main.tsx"),
                "exports-pkg",
            )
            .expect("package exports root should resolve");
        assert_eq!(
            package_module.resolved_specifier,
            "/workspace/node_modules/exports-pkg/esm/root.js"
        );
        let package_subpath = host
            .resolve_runtime_module(
                &runtime_context.context_id,
                Some("/workspace/src/main.tsx"),
                "exports-pkg/feature",
            )
            .expect("package exports subpath should resolve");
        assert_eq!(
            package_subpath.resolved_specifier,
            "/workspace/node_modules/exports-pkg/feature.cjs"
        );
        let browser_entry = host
            .resolve_runtime_module(
                &runtime_context.context_id,
                Some("/workspace/src/main.tsx"),
                "browser-pkg",
            )
            .expect("browser field should remap package entry");
        assert_eq!(
            browser_entry.resolved_specifier,
            "/workspace/node_modules/browser-pkg/browser.js"
        );
        let browser_relative = host
            .resolve_runtime_module(
                &runtime_context.context_id,
                Some("/workspace/node_modules/browser-pkg/server.js"),
                "./feature.js",
            )
            .expect("browser object mapping should remap package relative imports");
        assert_eq!(
            browser_relative.resolved_specifier,
            "/workspace/node_modules/browser-pkg/feature-browser.js"
        );
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
                        String::from("/workspace/node_modules/browser-pkg/package.json"),
                        String::from("/workspace/node_modules/exports-pkg/package.json"),
                        String::from("/workspace/package.json"),
                        String::from("/workspace/src/main.tsx"),
                    ]
                && response_descriptor
                    == PreviewResponseDescriptor {
                        kind: PreviewResponseKind::WorkspaceAsset,
                        workspace_path: Some(String::from("/workspace/src/main.tsx")),
                        document_root: Some(String::from("/workspace")),
                        hydrate_paths: vec![
                            String::from("/workspace/node_modules/browser-pkg/package.json"),
                            String::from("/workspace/node_modules/exports-pkg/package.json"),
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
                    && timers.len() == 3
                    && timers.iter().all(|timer| timer.timer_id == "runtime-timer-2"
                        && timer.kind == HostRuntimeTimerKind::Interval)
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
