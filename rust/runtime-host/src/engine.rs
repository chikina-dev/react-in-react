#[cfg(feature = "quickjs-ng-engine")]
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
#[cfg(feature = "quickjs-ng-engine")]
use std::ffi::{CStr, CString, c_void};
#[cfg(feature = "quickjs-ng-engine")]
use std::ptr;

use crate::protocol::{
    HostRuntimeBootstrapPlan, HostRuntimeEvent, HostRuntimeModuleImportPlan,
    HostRuntimeModuleLoaderPlan, HostRuntimeTimer, RunCommandKind, RunPlan, RunRequest,
};
#[cfg(feature = "quickjs-ng-engine")]
use crate::protocol::{HostRuntimeConsoleLevel, HostRuntimeTimerKind};
#[cfg(feature = "quickjs-ng-engine")]
use crate::protocol::{HostRuntimeModuleFormat, WorkspaceEntryKind, WorkspaceEntrySummary};
use crate::vfs::VirtualFileSystem;
#[cfg(feature = "quickjs-ng-engine")]
use crate::vfs::normalize_posix_path;
#[cfg(feature = "quickjs-ng-engine")]
use quickjs_ng_sys as qjs;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EngineDescriptor {
    pub name: &'static str,
    pub supports_interrupts: bool,
    pub supports_module_loader: bool,
    pub supports_eval: bool,
    pub supports_job_queue: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSessionSpec {
    pub session_id: String,
    pub workspace_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineSessionHandle {
    pub engine_session_id: String,
    pub workspace_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineContextSpec {
    pub context_id: String,
    pub session_id: String,
    pub engine_session_id: String,
    pub cwd: String,
    pub entrypoint: String,
    pub argv_len: usize,
    pub env_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineContextHandle {
    pub engine_session_id: String,
    pub engine_context_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineContextState {
    Booted,
    Ready,
    Disposed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineContextSnapshot {
    pub engine_session_id: String,
    pub engine_context_id: String,
    pub session_id: String,
    pub cwd: String,
    pub entrypoint: String,
    pub argv_len: usize,
    pub env_count: usize,
    pub pending_jobs: usize,
    pub registered_modules: usize,
    pub bootstrap_specifier: Option<String>,
    pub module_loader_roots: Vec<String>,
    pub state: EngineContextState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineEvalMode {
    Script,
    Module,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineEvalRequest {
    pub filename: String,
    pub source: String,
    pub mode: EngineEvalMode,
}

#[derive(Debug, Clone)]
pub struct EngineBootstrapBridge {
    pub cwd: String,
    pub argv: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub vfs: VirtualFileSystem,
}

#[derive(Debug, Clone)]
pub struct EngineBridgeSnapshot {
    pub cwd: String,
    pub vfs: VirtualFileSystem,
    pub events: Vec<HostRuntimeEvent>,
    pub timers: Vec<HostRuntimeTimer>,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineEvalOutcome {
    pub result_summary: String,
    pub pending_jobs: usize,
    pub state: EngineContextState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineJobDrain {
    pub drained_jobs: usize,
    pub pending_jobs: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineRegisteredModule {
    pub specifier: String,
    pub source: String,
}

pub trait EngineAdapter {
    fn descriptor(&self) -> EngineDescriptor;

    fn plan_run(&self, request: &RunRequest) -> RunPlan;

    fn boot_session(&mut self, spec: &EngineSessionSpec) -> Result<EngineSessionHandle, String>;

    fn dispose_session(&mut self, handle: &EngineSessionHandle);

    fn create_context(&mut self, spec: &EngineContextSpec) -> Result<EngineContextHandle, String>;

    fn describe_context(&self, handle: &EngineContextHandle) -> Option<EngineContextSnapshot>;

    fn eval(
        &mut self,
        handle: &EngineContextHandle,
        request: &EngineEvalRequest,
    ) -> Result<EngineEvalOutcome, String>;

    fn bootstrap(
        &mut self,
        handle: &EngineContextHandle,
        plan: &HostRuntimeBootstrapPlan,
        loader_plan: &HostRuntimeModuleLoaderPlan,
        import_plans: &[HostRuntimeModuleImportPlan],
        _bridge: &EngineBootstrapBridge,
    ) -> Result<EngineEvalOutcome, String>;

    fn list_modules(
        &self,
        handle: &EngineContextHandle,
    ) -> Result<Vec<EngineRegisteredModule>, String>;

    fn read_module(
        &self,
        handle: &EngineContextHandle,
        specifier: &str,
    ) -> Result<EngineRegisteredModule, String>;

    fn take_bridge_snapshot(
        &mut self,
        handle: &EngineContextHandle,
    ) -> Result<Option<EngineBridgeSnapshot>, String>;

    fn fire_timers(
        &mut self,
        handle: &EngineContextHandle,
        now_ms: u64,
        timer_ids: &[String],
    ) -> Result<usize, String>;

    fn drain_jobs(&mut self, handle: &EngineContextHandle) -> Result<EngineJobDrain, String>;

    fn interrupt(&mut self, handle: &EngineContextHandle, reason: &str) -> Result<(), String>;

    fn dispose_context(&mut self, handle: &EngineContextHandle);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EngineContextRecord {
    session_id: String,
    engine_session_id: String,
    cwd: String,
    entrypoint: String,
    argv_len: usize,
    env_count: usize,
    pending_jobs: usize,
    registered_modules: usize,
    bootstrap_specifier: Option<String>,
    module_loader_roots: Vec<String>,
    modules: BTreeMap<String, String>,
    state: EngineContextState,
}

#[derive(Debug, Default)]
struct EngineStateStore {
    sessions: BTreeMap<String, EngineSessionHandle>,
    contexts: BTreeMap<String, EngineContextRecord>,
}

impl EngineStateStore {
    fn boot_session(
        &mut self,
        spec: &EngineSessionSpec,
        session_prefix: &str,
    ) -> EngineSessionHandle {
        let handle = EngineSessionHandle {
            engine_session_id: format!("{session_prefix}:{}", spec.session_id),
            workspace_root: spec.workspace_root.clone(),
        };
        self.sessions
            .insert(handle.engine_session_id.clone(), handle.clone());
        handle
    }

    fn dispose_session(&mut self, handle: &EngineSessionHandle) {
        self.sessions.remove(&handle.engine_session_id);
        self.contexts
            .retain(|_, context| context.engine_session_id != handle.engine_session_id);
    }

    fn create_context(
        &mut self,
        spec: &EngineContextSpec,
        context_prefix: &str,
        engine_label: &str,
    ) -> Result<EngineContextHandle, String> {
        if !self.sessions.contains_key(&spec.engine_session_id) {
            return Err(format!(
                "{engine_label} session not booted: {}",
                spec.engine_session_id
            ));
        }

        let handle = EngineContextHandle {
            engine_session_id: spec.engine_session_id.clone(),
            engine_context_id: format!("{context_prefix}:{}", spec.context_id),
        };
        self.contexts.insert(
            handle.engine_context_id.clone(),
            EngineContextRecord {
                session_id: spec.session_id.clone(),
                engine_session_id: spec.engine_session_id.clone(),
                cwd: spec.cwd.clone(),
                entrypoint: spec.entrypoint.clone(),
                argv_len: spec.argv_len,
                env_count: spec.env_count,
                pending_jobs: 0,
                registered_modules: 0,
                bootstrap_specifier: None,
                module_loader_roots: Vec::new(),
                modules: BTreeMap::new(),
                state: EngineContextState::Booted,
            },
        );
        Ok(handle)
    }

    fn describe_context(&self, handle: &EngineContextHandle) -> Option<EngineContextSnapshot> {
        let context = self.contexts.get(&handle.engine_context_id)?;

        Some(EngineContextSnapshot {
            engine_session_id: handle.engine_session_id.clone(),
            engine_context_id: handle.engine_context_id.clone(),
            session_id: context.session_id.clone(),
            cwd: context.cwd.clone(),
            entrypoint: context.entrypoint.clone(),
            argv_len: context.argv_len,
            env_count: context.env_count,
            pending_jobs: context.pending_jobs,
            registered_modules: context.registered_modules,
            bootstrap_specifier: context.bootstrap_specifier.clone(),
            module_loader_roots: context.module_loader_roots.clone(),
            state: context.state.clone(),
        })
    }

    fn register_bootstrap(
        &mut self,
        handle: &EngineContextHandle,
        plan: &HostRuntimeBootstrapPlan,
        loader_plan: &HostRuntimeModuleLoaderPlan,
        import_plans: &[HostRuntimeModuleImportPlan],
        engine_label: &str,
    ) -> Result<(), String> {
        let context = self
            .contexts
            .get_mut(&handle.engine_context_id)
            .ok_or_else(|| {
                format!(
                    "{engine_label} context not found: {}",
                    handle.engine_context_id
                )
            })?;

        context.bootstrap_specifier = Some(plan.bootstrap_specifier.clone());
        context.module_loader_roots = loader_plan.node_module_search_roots.clone();
        context.modules = plan
            .modules
            .iter()
            .map(|module| (module.specifier.clone(), module.source.clone()))
            .chain(import_plans.iter().map(|plan| {
                (
                    plan.loaded_module.resolved_specifier.clone(),
                    plan.loaded_module.source.clone(),
                )
            }))
            .collect();
        context.registered_modules = context.modules.len();

        Ok(())
    }

    fn list_modules(
        &self,
        handle: &EngineContextHandle,
        engine_label: &str,
    ) -> Result<Vec<EngineRegisteredModule>, String> {
        let context = self
            .contexts
            .get(&handle.engine_context_id)
            .ok_or_else(|| {
                format!(
                    "{engine_label} context not found: {}",
                    handle.engine_context_id
                )
            })?;

        Ok(context
            .modules
            .iter()
            .map(|(specifier, source)| EngineRegisteredModule {
                specifier: specifier.clone(),
                source: source.clone(),
            })
            .collect())
    }

    fn read_module(
        &self,
        handle: &EngineContextHandle,
        specifier: &str,
        engine_label: &str,
    ) -> Result<EngineRegisteredModule, String> {
        let context = self
            .contexts
            .get(&handle.engine_context_id)
            .ok_or_else(|| {
                format!(
                    "{engine_label} context not found: {}",
                    handle.engine_context_id
                )
            })?;

        let source = context.modules.get(specifier).ok_or_else(|| {
            format!(
                "{engine_label} module not found in context {}: {specifier}",
                handle.engine_context_id
            )
        })?;

        Ok(EngineRegisteredModule {
            specifier: specifier.to_string(),
            source: source.clone(),
        })
    }

    fn mark_ready(
        &mut self,
        handle: &EngineContextHandle,
        engine_label: &str,
    ) -> Result<EngineEvalOutcome, String> {
        let context = self
            .contexts
            .get_mut(&handle.engine_context_id)
            .ok_or_else(|| {
                format!(
                    "{engine_label} context not found: {}",
                    handle.engine_context_id
                )
            })?;

        context.state = EngineContextState::Ready;

        Ok(EngineEvalOutcome {
            result_summary: String::new(),
            pending_jobs: context.pending_jobs,
            state: context.state.clone(),
        })
    }

    fn drain_jobs(
        &mut self,
        handle: &EngineContextHandle,
        engine_label: &str,
    ) -> Result<EngineJobDrain, String> {
        let context = self
            .contexts
            .get_mut(&handle.engine_context_id)
            .ok_or_else(|| {
                format!(
                    "{engine_label} context not found: {}",
                    handle.engine_context_id
                )
            })?;
        let drained_jobs = context.pending_jobs;
        context.pending_jobs = 0;

        Ok(EngineJobDrain {
            drained_jobs,
            pending_jobs: context.pending_jobs,
        })
    }

    fn interrupt(
        &mut self,
        handle: &EngineContextHandle,
        engine_label: &str,
    ) -> Result<(), String> {
        if self.contexts.contains_key(&handle.engine_context_id) {
            Ok(())
        } else {
            Err(format!(
                "{engine_label} context not found: {}",
                handle.engine_context_id
            ))
        }
    }

    fn dispose_context(&mut self, handle: &EngineContextHandle) {
        if let Some(context) = self.contexts.get_mut(&handle.engine_context_id) {
            context.state = EngineContextState::Disposed;
        }
        self.contexts.remove(&handle.engine_context_id);
    }
}

#[derive(Debug, Default)]
pub struct NullEngineAdapter {
    state: EngineStateStore,
}

#[derive(Default)]
pub struct QuickJsNgEngineAdapter {
    state: EngineStateStore,
    #[cfg(feature = "quickjs-ng-engine")]
    native: QuickJsNativeStore,
}

#[cfg(feature = "quickjs-ng-engine")]
struct QuickJsNativeSessionRecord {
    runtime: *mut qjs::JSRuntime,
    opaque: Box<QuickJsRuntimeOpaque>,
}

#[cfg(feature = "quickjs-ng-engine")]
#[derive(Debug)]
struct QuickJsNativeContextRecord {
    engine_session_id: String,
    runtime: *mut qjs::JSRuntime,
    context: *mut qjs::JSContext,
}

#[cfg(feature = "quickjs-ng-engine")]
#[derive(Default)]
struct QuickJsNativeStore {
    sessions: BTreeMap<String, QuickJsNativeSessionRecord>,
    contexts: BTreeMap<String, QuickJsNativeContextRecord>,
}

#[cfg(feature = "quickjs-ng-engine")]
#[derive(Default)]
struct QuickJsRuntimeOpaque {
    active_context_id: Option<String>,
    modules_by_context: BTreeMap<String, BTreeMap<String, String>>,
    aliases_by_context: BTreeMap<String, BTreeMap<(String, String), String>>,
    bridges_by_context: BTreeMap<String, QuickJsRuntimeBridge>,
}

#[cfg(feature = "quickjs-ng-engine")]
struct QuickJsRuntimeBridge {
    cwd: String,
    argv: Vec<String>,
    env: BTreeMap<String, String>,
    vfs: VirtualFileSystem,
    clock_ms: u64,
    next_timer_id: u64,
    timers: BTreeMap<String, QuickJsStoredTimer>,
    pending_events: Vec<HostRuntimeEvent>,
    exit_code: Option<i32>,
}

#[cfg(feature = "quickjs-ng-engine")]
struct QuickJsStoredTimer {
    timer: HostRuntimeTimer,
    callback_id: String,
}

#[cfg(feature = "quickjs-ng-engine")]
impl Clone for QuickJsRuntimeBridge {
    fn clone(&self) -> Self {
        Self {
            cwd: self.cwd.clone(),
            argv: self.argv.clone(),
            env: self.env.clone(),
            vfs: self.vfs.clone(),
            clock_ms: self.clock_ms,
            next_timer_id: self.next_timer_id,
            timers: BTreeMap::new(),
            pending_events: self.pending_events.clone(),
            exit_code: self.exit_code,
        }
    }
}

#[cfg(feature = "quickjs-ng-engine")]
impl QuickJsNativeStore {
    fn boot_session(&mut self, handle: &EngineSessionHandle) -> Result<(), String> {
        let runtime = unsafe { qjs::JS_NewRuntime() };
        if runtime.is_null() {
            return Err(format!(
                "quickjs-ng failed to create runtime for {}",
                handle.engine_session_id
            ));
        }

        let mut opaque = Box::new(QuickJsRuntimeOpaque::default());
        let opaque_ptr = opaque.as_mut() as *mut QuickJsRuntimeOpaque as *mut c_void;
        unsafe {
            qjs::JS_SetRuntimeOpaque(runtime, opaque_ptr);
            qjs::JS_SetModuleLoaderFunc(
                runtime,
                Some(quickjs_native_module_normalize),
                Some(quickjs_native_module_loader),
                opaque_ptr,
            );
        }

        self.sessions.insert(
            handle.engine_session_id.clone(),
            QuickJsNativeSessionRecord { runtime, opaque },
        );
        Ok(())
    }

    fn create_context(&mut self, handle: &EngineContextHandle) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&handle.engine_session_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng runtime session not found: {}",
                    handle.engine_session_id
                )
            })?;

        let context = unsafe { qjs::JS_NewContext(session.runtime) };
        if context.is_null() {
            return Err(format!(
                "quickjs-ng failed to create context for {}",
                handle.engine_context_id
            ));
        }

        quickjs_install_runtime_bridge(context)?;

        self.contexts.insert(
            handle.engine_context_id.clone(),
            QuickJsNativeContextRecord {
                engine_session_id: handle.engine_session_id.clone(),
                runtime: session.runtime,
                context,
            },
        );
        Ok(())
    }

    fn register_bootstrap(
        &mut self,
        handle: &EngineContextHandle,
        plan: &HostRuntimeBootstrapPlan,
        import_plans: &[HostRuntimeModuleImportPlan],
        bridge: &EngineBootstrapBridge,
    ) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&handle.engine_session_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng runtime session not found: {}",
                    handle.engine_session_id
                )
            })?;
        session.opaque.modules_by_context.insert(
            handle.engine_context_id.clone(),
            plan.modules
                .iter()
                .map(|module| (module.specifier.clone(), module.source.clone()))
                .chain(import_plans.iter().map(|plan| {
                    let child_imports = import_plans
                        .iter()
                        .filter(|child| {
                            child.importer.as_deref()
                                == Some(plan.loaded_module.resolved_specifier.as_str())
                        })
                        .map(|child| {
                            (
                                child.request_specifier.as_str(),
                                child.resolved_module.resolved_specifier.as_str(),
                            )
                        })
                        .collect::<Vec<_>>();
                    (
                        plan.loaded_module.resolved_specifier.clone(),
                        quickjs_materialize_module_source(
                            &plan.loaded_module.resolved_specifier,
                            &plan.loaded_module.source,
                            &plan.loaded_module.format,
                            &child_imports,
                        ),
                    )
                }))
                .collect(),
        );
        session.opaque.aliases_by_context.insert(
            handle.engine_context_id.clone(),
            import_plans
                .iter()
                .filter_map(|plan| {
                    plan.importer.as_ref().map(|importer| {
                        (
                            (importer.clone(), plan.request_specifier.clone()),
                            plan.resolved_module.resolved_specifier.clone(),
                        )
                    })
                })
                .collect(),
        );
        session.opaque.bridges_by_context.insert(
            handle.engine_context_id.clone(),
            QuickJsRuntimeBridge {
                cwd: bridge.cwd.clone(),
                argv: bridge.argv.clone(),
                env: bridge.env.clone(),
                vfs: bridge.vfs.clone(),
                clock_ms: 0,
                next_timer_id: 1,
                timers: BTreeMap::new(),
                pending_events: Vec::new(),
                exit_code: None,
            },
        );
        Ok(())
    }

    fn activate_context(&mut self, handle: &EngineContextHandle) -> Result<(), String> {
        let session = self
            .sessions
            .get_mut(&handle.engine_session_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng runtime session not found: {}",
                    handle.engine_session_id
                )
            })?;
        session.opaque.active_context_id = Some(handle.engine_context_id.clone());
        Ok(())
    }

    fn eval(
        &mut self,
        handle: &EngineContextHandle,
        request: &EngineEvalRequest,
    ) -> Result<(String, usize), String> {
        let context = self
            .contexts
            .get(&handle.engine_context_id)
            .ok_or_else(|| format!("quickjs-ng context not found: {}", handle.engine_context_id))?;

        let source = CString::new(request.source.as_str()).map_err(|_| {
            format!(
                "quickjs-ng source contains interior NUL bytes: {}",
                request.filename
            )
        })?;
        let filename = CString::new(request.filename.as_str()).map_err(|_| {
            format!(
                "quickjs-ng filename contains interior NUL bytes: {}",
                request.filename
            )
        })?;

        let eval_flags = match request.mode {
            EngineEvalMode::Script => (qjs::JS_EVAL_TYPE_GLOBAL | qjs::JS_EVAL_FLAG_STRICT) as i32,
            EngineEvalMode::Module => (qjs::JS_EVAL_TYPE_MODULE | qjs::JS_EVAL_FLAG_STRICT) as i32,
        };

        let value = unsafe {
            qjs::JS_Eval(
                context.context,
                source.as_ptr(),
                request.source.len() as qjs::size_t,
                filename.as_ptr(),
                eval_flags,
            )
        };

        if unsafe { qjs::JS_IsException(value) } {
            unsafe {
                qjs::JS_FreeValue(context.context, value);
            }
            return Err(format!(
                "quickjs-ng eval failed for {}: {}",
                request.filename,
                self.take_exception_string(context.context)
            ));
        }

        let summary = self.value_to_string(context.context, value);
        unsafe {
            qjs::JS_FreeValue(context.context, value);
        }

        let pending_jobs = usize::from(unsafe { qjs::JS_IsJobPending(context.runtime) });
        Ok((summary, pending_jobs))
    }

    fn list_modules(
        &self,
        handle: &EngineContextHandle,
    ) -> Result<Vec<EngineRegisteredModule>, String> {
        let session = self
            .sessions
            .get(&handle.engine_session_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng runtime session not found: {}",
                    handle.engine_session_id
                )
            })?;
        let modules = session
            .opaque
            .modules_by_context
            .get(&handle.engine_context_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng module registry not found for context {}",
                    handle.engine_context_id
                )
            })?;

        Ok(modules
            .iter()
            .map(|(specifier, source)| EngineRegisteredModule {
                specifier: specifier.clone(),
                source: source.clone(),
            })
            .collect())
    }

    fn read_module(
        &self,
        handle: &EngineContextHandle,
        specifier: &str,
    ) -> Result<EngineRegisteredModule, String> {
        let session = self
            .sessions
            .get(&handle.engine_session_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng runtime session not found: {}",
                    handle.engine_session_id
                )
            })?;
        let modules = session
            .opaque
            .modules_by_context
            .get(&handle.engine_context_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng module registry not found for context {}",
                    handle.engine_context_id
                )
            })?;
        let source = modules.get(specifier).ok_or_else(|| {
            format!(
                "quickjs-ng module not found in context {}: {}",
                handle.engine_context_id, specifier
            )
        })?;

        Ok(EngineRegisteredModule {
            specifier: specifier.to_string(),
            source: source.clone(),
        })
    }

    fn take_bridge_snapshot(
        &mut self,
        handle: &EngineContextHandle,
    ) -> Result<Option<EngineBridgeSnapshot>, String> {
        let session = self
            .sessions
            .get_mut(&handle.engine_session_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng runtime session not found: {}",
                    handle.engine_session_id
                )
            })?;
        let Some(bridge) = session
            .opaque
            .bridges_by_context
            .get_mut(&handle.engine_context_id)
        else {
            return Ok(None);
        };

        Ok(Some(EngineBridgeSnapshot {
            cwd: bridge.cwd.clone(),
            vfs: bridge.vfs.clone(),
            events: std::mem::take(&mut bridge.pending_events),
            timers: bridge
                .timers
                .values()
                .map(|entry| entry.timer.clone())
                .collect(),
            exit_code: bridge.exit_code,
        }))
    }

    fn fire_timers(
        &mut self,
        handle: &EngineContextHandle,
        now_ms: u64,
        timer_ids: &[String],
    ) -> Result<usize, String> {
        let context = self
            .contexts
            .get(&handle.engine_context_id)
            .ok_or_else(|| format!("quickjs-ng context not found: {}", handle.engine_context_id))?;
        let session = self
            .sessions
            .get_mut(&handle.engine_session_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng runtime session not found: {}",
                    handle.engine_session_id
                )
            })?;
        let bridge = session
            .opaque
            .bridges_by_context
            .get_mut(&handle.engine_context_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng runtime bridge missing context {}",
                    handle.engine_context_id
                )
            })?;
        bridge.clock_ms = now_ms;

        let mut fired = 0usize;
        for timer_id in timer_ids {
            let Some(mut stored_timer) = bridge.timers.remove(timer_id) else {
                continue;
            };
            let global = unsafe { qjs::JS_GetGlobalObject(context.context) };
            let runtime_value =
                unsafe { qjs::JS_GetPropertyStr(context.context, global, c"__runtime".as_ptr()) };
            let fire_timer = unsafe {
                qjs::JS_GetPropertyStr(context.context, runtime_value, c"fireTimer".as_ptr())
            };
            let callback_id = quickjs_new_string(context.context, &stored_timer.callback_id);
            let repeat_value =
                quickjs_new_bool(stored_timer.timer.kind == HostRuntimeTimerKind::Interval);
            let mut args = [callback_id, repeat_value];
            let result = unsafe {
                qjs::JS_Call(
                    context.context,
                    fire_timer,
                    runtime_value,
                    args.len() as i32,
                    args.as_mut_ptr(),
                )
            };
            unsafe {
                qjs::JS_FreeValue(context.context, fire_timer);
                qjs::JS_FreeValue(context.context, runtime_value);
                qjs::JS_FreeValue(context.context, global);
            }
            if unsafe { qjs::JS_IsException(result) } {
                unsafe {
                    qjs::JS_FreeValue(context.context, result);
                }
                return Err(format!(
                    "quickjs-ng timer callback failed: {}",
                    self.take_exception_string(context.context)
                ));
            }
            let keep_alive = unsafe { qjs::JS_ToBool(context.context, result) != 0 };
            unsafe {
                qjs::JS_FreeValue(context.context, result);
            }
            fired += 1;

            if stored_timer.timer.kind == HostRuntimeTimerKind::Interval && keep_alive {
                let step = stored_timer.timer.delay_ms.max(1);
                while stored_timer.timer.due_at_ms <= now_ms {
                    stored_timer.timer.due_at_ms =
                        stored_timer.timer.due_at_ms.saturating_add(step);
                }
                bridge.timers.insert(timer_id.clone(), stored_timer);
            }
        }

        Ok(fired)
    }

    fn drain_jobs(&mut self, handle: &EngineContextHandle) -> Result<EngineJobDrain, String> {
        let context = self
            .contexts
            .get(&handle.engine_context_id)
            .ok_or_else(|| format!("quickjs-ng context not found: {}", handle.engine_context_id))?;

        let mut drained_jobs = 0usize;
        while unsafe { qjs::JS_IsJobPending(context.runtime) } {
            let mut pending_ctx: *mut qjs::JSContext = ptr::null_mut();
            let status = unsafe { qjs::JS_ExecutePendingJob(context.runtime, &mut pending_ctx) };
            if status < 0 {
                let error_ctx = if pending_ctx.is_null() {
                    context.context
                } else {
                    pending_ctx
                };
                return Err(format!(
                    "quickjs-ng job drain failed: {}",
                    self.take_exception_string(error_ctx)
                ));
            }
            if status == 0 {
                break;
            }
            drained_jobs += status as usize;
        }

        Ok(EngineJobDrain {
            drained_jobs,
            pending_jobs: usize::from(unsafe { qjs::JS_IsJobPending(context.runtime) }),
        })
    }

    fn interrupt(&self, handle: &EngineContextHandle) -> Result<(), String> {
        if self.contexts.contains_key(&handle.engine_context_id) {
            Ok(())
        } else {
            Err(format!(
                "quickjs-ng context not found: {}",
                handle.engine_context_id
            ))
        }
    }

    fn dispose_context(&mut self, handle: &EngineContextHandle) {
        if let Some(context) = self.contexts.remove(&handle.engine_context_id) {
            if let Some(session) = self.sessions.get_mut(&context.engine_session_id) {
                session
                    .opaque
                    .modules_by_context
                    .remove(&handle.engine_context_id);
                session
                    .opaque
                    .aliases_by_context
                    .remove(&handle.engine_context_id);
                session
                    .opaque
                    .bridges_by_context
                    .remove(&handle.engine_context_id)
                    .into_iter()
                    .for_each(drop);
                if session.opaque.active_context_id.as_deref()
                    == Some(handle.engine_context_id.as_str())
                {
                    session.opaque.active_context_id = None;
                }
            }
            unsafe {
                qjs::JS_FreeContext(context.context);
            }
        }
    }

    fn dispose_session(&mut self, handle: &EngineSessionHandle) {
        let context_ids = self
            .contexts
            .iter()
            .filter_map(|(context_id, context)| {
                (context.engine_session_id == handle.engine_session_id).then(|| context_id.clone())
            })
            .collect::<Vec<_>>();

        for context_id in context_ids {
            if let Some(context) = self.contexts.remove(&context_id) {
                unsafe {
                    qjs::JS_FreeContext(context.context);
                }
            }
        }

        if let Some(session) = self.sessions.remove(&handle.engine_session_id) {
            unsafe {
                qjs::JS_SetRuntimeOpaque(session.runtime, ptr::null_mut());
            }
            unsafe {
                qjs::JS_FreeRuntime(session.runtime);
            }
        }
    }

    fn take_exception_string(&self, context: *mut qjs::JSContext) -> String {
        let exception = unsafe { qjs::JS_GetException(context) };
        if unsafe { qjs::JS_IsException(exception) } {
            unsafe {
                qjs::JS_FreeValue(context, exception);
            }
            return "unknown quickjs-ng exception".into();
        }

        let message = self.value_to_string(context, exception);
        unsafe {
            qjs::JS_FreeValue(context, exception);
        }
        message
    }

    fn value_to_string(&self, context: *mut qjs::JSContext, value: qjs::JSValue) -> String {
        let pointer = unsafe { qjs::JS_ToCString(context, value) };
        if pointer.is_null() {
            return "<non-string js value>".into();
        }

        let rendered = unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned();
        unsafe {
            qjs::JS_FreeCString(context, pointer);
        }
        rendered
    }
}

#[cfg(feature = "quickjs-ng-engine")]
unsafe extern "C" fn quickjs_native_module_normalize(
    ctx: *mut qjs::JSContext,
    module_base_name: *const ::core::ffi::c_char,
    module_name: *const ::core::ffi::c_char,
    opaque: *mut c_void,
) -> *mut ::core::ffi::c_char {
    let Some(runtime) = quickjs_runtime_opaque(opaque) else {
        return ptr::null_mut();
    };
    let requested = unsafe { CStr::from_ptr(module_name) }
        .to_string_lossy()
        .into_owned();

    let importer = (!module_base_name.is_null()).then(|| unsafe {
        CStr::from_ptr(module_base_name)
            .to_string_lossy()
            .into_owned()
    });

    let resolved = if let Some(base_name) = importer.clone() {
        runtime
            .active_context_id
            .as_ref()
            .and_then(|context_id| runtime.aliases_by_context.get(context_id))
            .and_then(|aliases| {
                aliases
                    .get(&(base_name.clone(), requested.clone()))
                    .cloned()
            })
            .or_else(|| {
                runtime.active_context_id.as_ref().and_then(|context_id| {
                    quickjs_runtime_resolve_specifier(
                        runtime,
                        context_id,
                        Some(base_name.as_str()),
                        &requested,
                    )
                    .ok()
                })
            })
            .unwrap_or(requested)
    } else {
        runtime
            .active_context_id
            .as_ref()
            .and_then(|context_id| {
                quickjs_runtime_resolve_specifier(runtime, context_id, None, &requested).ok()
            })
            .unwrap_or(requested)
    };

    quickjs_strdup(ctx, &resolved)
}

#[cfg(feature = "quickjs-ng-engine")]
unsafe extern "C" fn quickjs_native_module_loader(
    ctx: *mut qjs::JSContext,
    module_name: *const ::core::ffi::c_char,
    opaque: *mut c_void,
) -> *mut qjs::JSModuleDef {
    let Some(runtime) = quickjs_runtime_opaque(opaque) else {
        return quickjs_throw_module_error(ctx, "quickjs-ng runtime opaque is missing");
    };

    let module_name = unsafe { CStr::from_ptr(module_name) }
        .to_string_lossy()
        .into_owned();
    let Some(active_context_id) = runtime.active_context_id.clone() else {
        return quickjs_throw_module_error(ctx, "quickjs-ng module loader has no active context");
    };
    let Some(modules) = runtime.modules_by_context.get(&active_context_id) else {
        return quickjs_throw_module_error(
            ctx,
            &format!("quickjs-ng module loader has no registry for context {active_context_id}"),
        );
    };
    let Some(source) = modules.get(&module_name) else {
        if quickjs_ensure_module_registered(runtime, &active_context_id, &module_name).is_err() {
            return quickjs_throw_module_error(
                ctx,
                &format!("quickjs-ng module not registered: {module_name}"),
            );
        }
        let Some(modules) = runtime.modules_by_context.get(&active_context_id) else {
            return quickjs_throw_module_error(
                ctx,
                &format!(
                    "quickjs-ng module loader has no registry for context {active_context_id}"
                ),
            );
        };
        let Some(source) = modules.get(&module_name) else {
            return quickjs_throw_module_error(
                ctx,
                &format!("quickjs-ng module not registered: {module_name}"),
            );
        };
        return quickjs_compile_registered_module(ctx, &module_name, source);
    };
    quickjs_compile_registered_module(ctx, &module_name, source)
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_compile_registered_module(
    ctx: *mut qjs::JSContext,
    module_name: &str,
    source: &str,
) -> *mut qjs::JSModuleDef {
    let Ok(filename) = CString::new(module_name) else {
        return quickjs_throw_module_error(
            ctx,
            &format!("quickjs-ng module name contains NUL bytes: {module_name}"),
        );
    };

    let value = unsafe {
        qjs::JS_Eval(
            ctx,
            source.as_ptr().cast(),
            source.len() as qjs::size_t,
            filename.as_ptr(),
            (qjs::JS_EVAL_TYPE_MODULE | qjs::JS_EVAL_FLAG_COMPILE_ONLY | qjs::JS_EVAL_FLAG_STRICT)
                as i32,
        )
    };
    if unsafe { qjs::JS_IsException(value) } {
        unsafe {
            qjs::JS_FreeValue(ctx, value);
        }
        return ptr::null_mut();
    }

    if unsafe { qjs::JS_VALUE_GET_TAG(value) } != qjs::JS_TAG_MODULE {
        unsafe {
            qjs::JS_FreeValue(ctx, value);
        }
        return quickjs_throw_module_error(
            ctx,
            &format!("quickjs-ng compiled a non-module value for {module_name}"),
        );
    }

    let module = unsafe { qjs::JS_VALUE_GET_PTR(value) }.cast::<qjs::JSModuleDef>();
    unsafe {
        qjs::JS_FreeValue(ctx, value);
    }
    if module.is_null() {
        return quickjs_throw_module_error(
            ctx,
            &format!("quickjs-ng returned a null module for {module_name}"),
        );
    }
    module
}

#[cfg(feature = "quickjs-ng-engine")]
unsafe extern "C" fn quickjs_runtime_invoke(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValue,
    argc: ::core::ffi::c_int,
    argv: *mut qjs::JSValue,
) -> qjs::JSValue {
    let Some(runtime) = quickjs_runtime_opaque_from_context(ctx) else {
        return quickjs_throw_value_error(ctx, "quickjs-ng runtime opaque is missing");
    };
    let Some(active_context_id) = runtime.active_context_id.clone() else {
        return quickjs_throw_value_error(ctx, "quickjs-ng runtime bridge has no active context");
    };
    let Some(bridge) = runtime.bridges_by_context.get_mut(&active_context_id) else {
        return quickjs_throw_value_error(
            ctx,
            &format!("quickjs-ng runtime bridge missing context {active_context_id}"),
        );
    };

    let arguments =
        unsafe { std::slice::from_raw_parts(argv, usize::try_from(argc).unwrap_or_default()) };
    let Some(kind_value) = arguments.first().copied() else {
        return quickjs_throw_value_error(ctx, "quickjs-ng runtime bridge requires a command");
    };
    let Ok(kind) = quickjs_value_to_string(ctx, kind_value) else {
        return quickjs_throw_value_error(
            ctx,
            "quickjs-ng runtime bridge command must be a string",
        );
    };
    let payload = arguments.get(1).copied().unwrap_or(qjs::JS_UNDEFINED);

    match kind.as_str() {
        "process.cwd" => quickjs_new_string(ctx, &bridge.cwd),
        "process.chdir" => {
            let Ok(path) = quickjs_get_required_string_property(ctx, payload, "path") else {
                return quickjs_throw_value_error(
                    ctx,
                    "quickjs-ng process.chdir requires a string path",
                );
            };
            let resolved = quickjs_bridge_resolve_path(&bridge.cwd, &path);
            if !bridge.vfs.exists(&resolved) || !bridge.vfs.is_dir(&resolved) {
                return quickjs_throw_value_error(
                    ctx,
                    &format!("quickjs-ng process.chdir target is not a directory: {resolved}"),
                );
            }
            bridge.cwd = resolved;
            qjs::JS_UNDEFINED
        }
        "process.exit" => {
            let code = quickjs_get_optional_i32_property(ctx, payload, "code").unwrap_or(0);
            bridge.exit_code = Some(code);
            bridge
                .pending_events
                .push(HostRuntimeEvent::ProcessExit { code });
            qjs::JS_UNDEFINED
        }
        "process.argv" => quickjs_new_string_array(ctx, &bridge.argv),
        "process.env" => quickjs_new_string_map(ctx, &bridge.env),
        "fs.exists" => {
            let Ok(path) = quickjs_get_required_string_property(ctx, payload, "path") else {
                return quickjs_throw_value_error(
                    ctx,
                    "quickjs-ng fs.exists requires a string path",
                );
            };
            let resolved = quickjs_bridge_resolve_path(&bridge.cwd, &path);
            let object = quickjs_new_object(ctx);
            if quickjs_set_property(
                ctx,
                object,
                "exists",
                quickjs_new_bool(bridge.vfs.exists(&resolved)),
            )
            .is_err()
            {
                return qjs::JS_EXCEPTION;
            }
            object
        }
        "fs.stat" => {
            let Ok(path) = quickjs_get_required_string_property(ctx, payload, "path") else {
                return quickjs_throw_value_error(ctx, "quickjs-ng fs.stat requires a string path");
            };
            let resolved = quickjs_bridge_resolve_path(&bridge.cwd, &path);
            let Some(entry) = bridge.vfs.stat(&resolved) else {
                return quickjs_throw_value_error(
                    ctx,
                    &format!("quickjs-ng fs.stat missing path: {resolved}"),
                );
            };
            let object = quickjs_new_object(ctx);
            let entry_value = quickjs_new_workspace_entry(ctx, &entry);
            if quickjs_set_property(ctx, object, "entry", entry_value).is_err() {
                return qjs::JS_EXCEPTION;
            }
            object
        }
        "fs.read-dir" => {
            let Ok(path) = quickjs_get_required_string_property(ctx, payload, "path") else {
                return quickjs_throw_value_error(
                    ctx,
                    "quickjs-ng fs.read-dir requires a string path",
                );
            };
            let resolved = quickjs_bridge_resolve_path(&bridge.cwd, &path);
            let Ok(entries) = bridge.vfs.read_dir(&resolved) else {
                return quickjs_throw_value_error(
                    ctx,
                    &format!("quickjs-ng fs.read-dir missing directory: {resolved}"),
                );
            };
            let object = quickjs_new_object(ctx);
            let entries_value = quickjs_new_workspace_entries(ctx, &entries);
            if quickjs_set_property(ctx, object, "entries", entries_value).is_err() {
                return qjs::JS_EXCEPTION;
            }
            object
        }
        "fs.read-file" => {
            let Ok(path) = quickjs_get_required_string_property(ctx, payload, "path") else {
                return quickjs_throw_value_error(
                    ctx,
                    "quickjs-ng fs.read-file requires a string path",
                );
            };
            let resolved = quickjs_bridge_resolve_path(&bridge.cwd, &path);
            let Some(file) = bridge.vfs.read(&resolved) else {
                return quickjs_throw_value_error(
                    ctx,
                    &format!("quickjs-ng fs.read-file missing path: {resolved}"),
                );
            };
            if file.is_text {
                quickjs_new_string(ctx, &String::from_utf8_lossy(&file.bytes))
            } else {
                unsafe {
                    qjs::JS_NewArrayBufferCopy(
                        ctx,
                        file.bytes.as_ptr(),
                        file.bytes.len() as qjs::size_t,
                    )
                }
            }
        }
        "fs.mkdir" => {
            let Ok(path) = quickjs_get_required_string_property(ctx, payload, "path") else {
                return quickjs_throw_value_error(
                    ctx,
                    "quickjs-ng fs.mkdir requires a string path",
                );
            };
            let resolved = quickjs_bridge_resolve_path(&bridge.cwd, &path);
            if let Err(error) = bridge.vfs.create_dir_all(&resolved) {
                return quickjs_throw_value_error(
                    ctx,
                    &format!("quickjs-ng fs.mkdir failed: {error}"),
                );
            }
            qjs::JS_UNDEFINED
        }
        "fs.write-file" => {
            let Ok(path) = quickjs_get_required_string_property(ctx, payload, "path") else {
                return quickjs_throw_value_error(
                    ctx,
                    "quickjs-ng fs.write-file requires a string path",
                );
            };
            let resolved = quickjs_bridge_resolve_path(&bridge.cwd, &path);
            let is_text =
                quickjs_get_optional_bool_property(ctx, payload, "isText").unwrap_or(false);
            let bytes_value = unsafe { qjs::JS_GetPropertyStr(ctx, payload, c"bytes".as_ptr()) };
            let bytes = quickjs_extract_bytes(ctx, bytes_value, is_text);
            unsafe {
                qjs::JS_FreeValue(ctx, bytes_value);
            }
            let Ok(bytes) = bytes else {
                return quickjs_throw_value_error(
                    ctx,
                    "quickjs-ng fs.write-file bytes must be string, Uint8Array, or ArrayBuffer",
                );
            };
            if let Err(error) = bridge.vfs.write_file(&resolved, bytes, is_text) {
                return quickjs_throw_value_error(
                    ctx,
                    &format!("quickjs-ng fs.write-file failed: {error}"),
                );
            }
            qjs::JS_UNDEFINED
        }
        "path.resolve" => quickjs_wrap_path_value(ctx, quickjs_path_resolve(ctx, bridge, payload)),
        "path.join" => quickjs_wrap_path_value(ctx, quickjs_path_join(ctx, payload)),
        "path.dirname" => quickjs_wrap_path_value(
            ctx,
            quickjs_get_required_string_property(ctx, payload, "path").map(|path| {
                let normalized = normalize_posix_path(&path);
                normalized
                    .rsplit_once('/')
                    .map(|(parent, _)| {
                        if parent.is_empty() {
                            "/".into()
                        } else {
                            parent.into()
                        }
                    })
                    .unwrap_or_else(|| ".".into())
            }),
        ),
        "path.basename" => quickjs_wrap_path_value(
            ctx,
            quickjs_get_required_string_property(ctx, payload, "path").map(|path| {
                let normalized = normalize_posix_path(&path);
                normalized
                    .rsplit('/')
                    .next()
                    .map(str::to_string)
                    .unwrap_or_else(|| normalized)
            }),
        ),
        "path.extname" => quickjs_wrap_path_value(
            ctx,
            quickjs_get_required_string_property(ctx, payload, "path").map(|path| {
                let normalized = normalize_posix_path(&path);
                let basename = normalized.rsplit('/').next().unwrap_or("");
                basename
                    .rsplit_once('.')
                    .map(|(_, ext)| format!(".{ext}"))
                    .unwrap_or_default()
            }),
        ),
        "path.normalize" => quickjs_wrap_path_value(
            ctx,
            quickjs_get_required_string_property(ctx, payload, "path")
                .map(|path| normalize_posix_path(&path)),
        ),
        "console.emit" => {
            let level = quickjs_get_optional_console_level_property(ctx, payload, "level")
                .unwrap_or(HostRuntimeConsoleLevel::Log);
            let values =
                quickjs_get_string_array_property(ctx, payload, "values").unwrap_or_default();
            let line = values.join(" ");
            bridge.pending_events.push(HostRuntimeEvent::Console {
                level: level.clone(),
                line: line.clone(),
            });
            bridge.pending_events.push(match level {
                HostRuntimeConsoleLevel::Warn | HostRuntimeConsoleLevel::Error => {
                    HostRuntimeEvent::Stderr { chunk: line }
                }
                HostRuntimeConsoleLevel::Log | HostRuntimeConsoleLevel::Info => {
                    HostRuntimeEvent::Stdout { chunk: line }
                }
            });
            qjs::JS_UNDEFINED
        }
        "timers.schedule" => {
            let delay_ms = quickjs_get_optional_i32_property(ctx, payload, "delayMs")
                .unwrap_or(0)
                .max(0) as u64;
            let repeat =
                quickjs_get_optional_bool_property(ctx, payload, "repeat").unwrap_or(false);
            let timer_id = format!("native-timer-{}", bridge.next_timer_id);
            bridge.next_timer_id += 1;
            if !repeat {
                let callback =
                    unsafe { qjs::JS_GetPropertyStr(ctx, payload, c"callback".as_ptr()) };
                if unsafe { qjs::JS_IsFunction(ctx, callback) } && delay_ms == 0 {
                    let result = unsafe {
                        qjs::JS_Call(ctx, callback, qjs::JS_UNDEFINED, 0, ptr::null_mut())
                    };
                    unsafe {
                        qjs::JS_FreeValue(ctx, callback);
                    }
                    if unsafe { qjs::JS_IsException(result) } {
                        unsafe {
                            qjs::JS_FreeValue(ctx, result);
                        }
                        return qjs::JS_EXCEPTION;
                    }
                    unsafe {
                        qjs::JS_FreeValue(ctx, result);
                    }
                } else {
                    unsafe {
                        qjs::JS_FreeValue(ctx, callback);
                    }
                }
            }
            if repeat || delay_ms > 0 {
                let Ok(callback_id) =
                    quickjs_get_required_string_property(ctx, payload, "callbackId")
                else {
                    return quickjs_throw_value_error(
                        ctx,
                        "quickjs-ng timers.schedule requires a callbackId for delayed timers",
                    );
                };
                bridge.timers.insert(
                    timer_id.clone(),
                    QuickJsStoredTimer {
                        timer: HostRuntimeTimer {
                            timer_id: timer_id.clone(),
                            kind: if repeat {
                                HostRuntimeTimerKind::Interval
                            } else {
                                HostRuntimeTimerKind::Timeout
                            },
                            delay_ms,
                            due_at_ms: bridge.clock_ms.saturating_add(delay_ms),
                        },
                        callback_id,
                    },
                );
            }
            quickjs_new_string(ctx, &timer_id)
        }
        "timers.clear" => {
            if let Ok(timer_id) = quickjs_get_required_string_property(ctx, payload, "timerId") {
                bridge.timers.remove(&timer_id);
            }
            qjs::JS_UNDEFINED
        }
        _ => quickjs_throw_value_error(
            ctx,
            &format!("quickjs-ng runtime bridge does not support {kind}"),
        ),
    }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_throw_module_error(ctx: *mut qjs::JSContext, message: &str) -> *mut qjs::JSModuleDef {
    let value =
        unsafe { qjs::JS_NewStringLen(ctx, message.as_ptr().cast(), message.len() as qjs::size_t) };
    unsafe {
        qjs::JS_Throw(ctx, value);
    }
    ptr::null_mut()
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_runtime_opaque(opaque: *mut c_void) -> Option<&'static mut QuickJsRuntimeOpaque> {
    (!opaque.is_null()).then(|| unsafe { &mut *opaque.cast::<QuickJsRuntimeOpaque>() })
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_runtime_opaque_from_context(
    ctx: *mut qjs::JSContext,
) -> Option<&'static mut QuickJsRuntimeOpaque> {
    let runtime = unsafe { qjs::JS_GetRuntime(ctx) };
    let opaque = unsafe { qjs::JS_GetRuntimeOpaque(runtime) };
    quickjs_runtime_opaque(opaque)
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_strdup(ctx: *mut qjs::JSContext, value: &str) -> *mut ::core::ffi::c_char {
    let size = value.len() + 1;
    let pointer = unsafe { qjs::js_malloc(ctx, size as qjs::size_t) }.cast::<u8>();
    if pointer.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        ptr::copy_nonoverlapping(value.as_ptr(), pointer, value.len());
        *pointer.add(value.len()) = 0;
    }
    pointer.cast()
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_install_runtime_bridge(ctx: *mut qjs::JSContext) -> Result<(), String> {
    let global = unsafe { qjs::JS_GetGlobalObject(ctx) };
    if unsafe { qjs::JS_IsException(global) } {
        return Err("quickjs-ng failed to access global object".into());
    }

    let runtime_object = quickjs_new_object(ctx);
    let invoke_name = CString::new("invoke").expect("static string should not contain NUL");
    let invoke = unsafe {
        qjs::JS_NewCFunction2(
            ctx,
            Some(quickjs_runtime_invoke),
            invoke_name.as_ptr(),
            2,
            qjs::JSCFunctionEnum_JS_CFUNC_generic,
            0,
        )
    };
    if quickjs_set_property(ctx, runtime_object, "invoke", invoke).is_err()
        || quickjs_set_property(ctx, global, "__runtime", runtime_object).is_err()
    {
        unsafe {
            qjs::JS_FreeValue(ctx, global);
        }
        return Err("quickjs-ng failed to install runtime bridge".into());
    }

    unsafe {
        qjs::JS_FreeValue(ctx, global);
    }
    Ok(())
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_new_object(ctx: *mut qjs::JSContext) -> qjs::JSValue {
    unsafe { qjs::JS_NewObject(ctx) }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_new_bool(value: bool) -> qjs::JSValue {
    if value { qjs::JS_TRUE } else { qjs::JS_FALSE }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_new_string(ctx: *mut qjs::JSContext, value: &str) -> qjs::JSValue {
    unsafe { qjs::JS_NewStringLen(ctx, value.as_ptr().cast(), value.len() as qjs::size_t) }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_new_string_array(ctx: *mut qjs::JSContext, values: &[String]) -> qjs::JSValue {
    let array = unsafe { qjs::JS_NewArray(ctx) };
    for (index, value) in values.iter().enumerate() {
        let item = quickjs_new_string(ctx, value);
        if unsafe { qjs::JS_SetPropertyUint32(ctx, array, index as u32, item) } < 0 {
            return qjs::JS_EXCEPTION;
        }
    }
    array
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_new_string_map(
    ctx: *mut qjs::JSContext,
    values: &BTreeMap<String, String>,
) -> qjs::JSValue {
    let object = quickjs_new_object(ctx);
    for (key, value) in values {
        if quickjs_set_property(ctx, object, key, quickjs_new_string(ctx, value)).is_err() {
            return qjs::JS_EXCEPTION;
        }
    }
    object
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_new_workspace_entry(
    ctx: *mut qjs::JSContext,
    entry: &WorkspaceEntrySummary,
) -> qjs::JSValue {
    let object = quickjs_new_object(ctx);
    let kind = match entry.kind {
        WorkspaceEntryKind::File => "file",
        WorkspaceEntryKind::Directory => "directory",
    };
    if quickjs_set_property(ctx, object, "path", quickjs_new_string(ctx, &entry.path)).is_err()
        || quickjs_set_property(ctx, object, "kind", quickjs_new_string(ctx, kind)).is_err()
        || quickjs_set_property(
            ctx,
            object,
            "size",
            qjs::JS_MKVAL(qjs::JS_TAG_INT, entry.size as i32),
        )
        .is_err()
        || quickjs_set_property(ctx, object, "isText", quickjs_new_bool(entry.is_text)).is_err()
    {
        return qjs::JS_EXCEPTION;
    }
    object
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_new_workspace_entries(
    ctx: *mut qjs::JSContext,
    entries: &[WorkspaceEntrySummary],
) -> qjs::JSValue {
    let array = unsafe { qjs::JS_NewArray(ctx) };
    for (index, entry) in entries.iter().enumerate() {
        let value = quickjs_new_workspace_entry(ctx, entry);
        if unsafe { qjs::JS_SetPropertyUint32(ctx, array, index as u32, value) } < 0 {
            return qjs::JS_EXCEPTION;
        }
    }
    array
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_wrap_path_value(
    ctx: *mut qjs::JSContext,
    value: Result<String, String>,
) -> qjs::JSValue {
    match value {
        Ok(value) => {
            let object = quickjs_new_object(ctx);
            if quickjs_set_property(ctx, object, "value", quickjs_new_string(ctx, &value)).is_err()
            {
                qjs::JS_EXCEPTION
            } else {
                object
            }
        }
        Err(message) => quickjs_throw_value_error(ctx, &message),
    }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_set_property(
    ctx: *mut qjs::JSContext,
    object: qjs::JSValue,
    key: &str,
    value: qjs::JSValue,
) -> Result<(), ()> {
    let key = CString::new(key).map_err(|_| ())?;
    if unsafe { qjs::JS_SetPropertyStr(ctx, object, key.as_ptr(), value) } < 0 {
        Err(())
    } else {
        Ok(())
    }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_throw_value_error(ctx: *mut qjs::JSContext, message: &str) -> qjs::JSValue {
    let value = quickjs_new_string(ctx, message);
    unsafe {
        qjs::JS_Throw(ctx, value);
    }
    qjs::JS_EXCEPTION
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_value_to_string(
    ctx: *mut qjs::JSContext,
    value: qjs::JSValue,
) -> Result<String, String> {
    let pointer = unsafe { qjs::JS_ToCString(ctx, value) };
    if pointer.is_null() {
        return Err("value is not coercible to string".into());
    }
    let rendered = unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned();
    unsafe {
        qjs::JS_FreeCString(ctx, pointer);
    }
    Ok(rendered)
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_get_required_string_property(
    ctx: *mut qjs::JSContext,
    payload: qjs::JSValue,
    key: &str,
) -> Result<String, String> {
    let key = CString::new(key).map_err(|_| "property name contains NUL".to_string())?;
    let value = unsafe { qjs::JS_GetPropertyStr(ctx, payload, key.as_ptr()) };
    let rendered = if unsafe { qjs::JS_IsUndefined(value) || qjs::JS_IsNull(value) } {
        Err(format!("missing property {key:?}"))
    } else {
        quickjs_value_to_string(ctx, value)
    };
    unsafe {
        qjs::JS_FreeValue(ctx, value);
    }
    rendered
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_get_optional_bool_property(
    ctx: *mut qjs::JSContext,
    payload: qjs::JSValue,
    key: &str,
) -> Option<bool> {
    let key = CString::new(key).ok()?;
    let value = unsafe { qjs::JS_GetPropertyStr(ctx, payload, key.as_ptr()) };
    let result = if unsafe { qjs::JS_IsUndefined(value) || qjs::JS_IsNull(value) } {
        None
    } else {
        Some(unsafe { qjs::JS_ToBool(ctx, value) != 0 })
    };
    unsafe {
        qjs::JS_FreeValue(ctx, value);
    }
    result
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_get_optional_i32_property(
    ctx: *mut qjs::JSContext,
    payload: qjs::JSValue,
    key: &str,
) -> Option<i32> {
    let key = CString::new(key).ok()?;
    let value = unsafe { qjs::JS_GetPropertyStr(ctx, payload, key.as_ptr()) };
    let result = if unsafe { qjs::JS_IsUndefined(value) || qjs::JS_IsNull(value) } {
        None
    } else {
        let mut output = 0;
        (unsafe { qjs::JS_ToInt32(ctx, &mut output, value) } == 0).then_some(output)
    };
    unsafe {
        qjs::JS_FreeValue(ctx, value);
    }
    result
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_get_optional_console_level_property(
    ctx: *mut qjs::JSContext,
    payload: qjs::JSValue,
    key: &str,
) -> Option<HostRuntimeConsoleLevel> {
    let rendered = quickjs_get_required_string_property(ctx, payload, key).ok()?;
    match rendered.as_str() {
        "info" => Some(HostRuntimeConsoleLevel::Info),
        "warn" => Some(HostRuntimeConsoleLevel::Warn),
        "error" => Some(HostRuntimeConsoleLevel::Error),
        "log" => Some(HostRuntimeConsoleLevel::Log),
        _ => None,
    }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_get_string_array_property(
    ctx: *mut qjs::JSContext,
    payload: qjs::JSValue,
    key: &str,
) -> Result<Vec<String>, String> {
    let key = CString::new(key).map_err(|_| "property name contains NUL".to_string())?;
    let array = unsafe { qjs::JS_GetPropertyStr(ctx, payload, key.as_ptr()) };
    if unsafe { qjs::JS_IsUndefined(array) || qjs::JS_IsNull(array) } {
        unsafe {
            qjs::JS_FreeValue(ctx, array);
        }
        return Ok(Vec::new());
    }
    if !unsafe { qjs::JS_IsArray(array) } {
        unsafe {
            qjs::JS_FreeValue(ctx, array);
        }
        return Err(format!("property {key:?} is not an array"));
    }
    let length_value = unsafe { qjs::JS_GetPropertyStr(ctx, array, c"length".as_ptr()) };
    let mut length = 0;
    let status = unsafe { qjs::JS_ToInt32(ctx, &mut length, length_value) };
    unsafe {
        qjs::JS_FreeValue(ctx, length_value);
    }
    if status != 0 {
        unsafe {
            qjs::JS_FreeValue(ctx, array);
        }
        return Err("array length is not numeric".into());
    }
    let mut result = Vec::with_capacity(length.max(0) as usize);
    for index in 0..length.max(0) {
        let item = unsafe { qjs::JS_GetPropertyUint32(ctx, array, index as u32) };
        let rendered = quickjs_value_to_string(ctx, item)?;
        unsafe {
            qjs::JS_FreeValue(ctx, item);
        }
        result.push(rendered);
    }
    unsafe {
        qjs::JS_FreeValue(ctx, array);
    }
    Ok(result)
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_extract_bytes(
    ctx: *mut qjs::JSContext,
    value: qjs::JSValue,
    is_text: bool,
) -> Result<Vec<u8>, String> {
    if unsafe { qjs::JS_IsString(value) } || is_text {
        return quickjs_value_to_string(ctx, value).map(|value| value.into_bytes());
    }

    if unsafe { qjs::JS_IsArrayBuffer(value) } {
        let mut size = 0;
        let pointer = unsafe { qjs::JS_GetArrayBuffer(ctx, &mut size, value) };
        if pointer.is_null() {
            return Err("array buffer could not be read".into());
        }
        return Ok(unsafe { std::slice::from_raw_parts(pointer, size as usize) }.to_vec());
    }

    let mut size = 0;
    let pointer = unsafe { qjs::JS_GetUint8Array(ctx, &mut size, value) };
    if !pointer.is_null() {
        return Ok(unsafe { std::slice::from_raw_parts(pointer, size as usize) }.to_vec());
    }

    Err("bytes must be string, ArrayBuffer, or Uint8Array".into())
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_bridge_resolve_path(cwd: &str, path: &str) -> String {
    if path.starts_with('/') {
        normalize_posix_path(path)
    } else {
        normalize_posix_path(&format!("{cwd}/{path}"))
    }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_path_resolve(
    ctx: *mut qjs::JSContext,
    bridge: &QuickJsRuntimeBridge,
    payload: qjs::JSValue,
) -> Result<String, String> {
    let segments = quickjs_get_string_array_property(ctx, payload, "segments")?;
    let mut current = bridge.cwd.clone();
    for segment in segments {
        if segment.starts_with('/') {
            current = normalize_posix_path(&segment);
        } else {
            current = normalize_posix_path(&format!("{current}/{segment}"));
        }
    }
    Ok(current)
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_path_join(ctx: *mut qjs::JSContext, payload: qjs::JSValue) -> Result<String, String> {
    let segments = quickjs_get_string_array_property(ctx, payload, "segments")?;
    if segments.is_empty() {
        return Ok(".".into());
    }
    Ok(normalize_posix_path(&segments.join("/")))
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_materialize_module_source(
    specifier: &str,
    source: &str,
    format: &HostRuntimeModuleFormat,
    child_imports: &[(&str, &str)],
) -> String {
    match format {
        HostRuntimeModuleFormat::Module => source.to_string(),
        HostRuntimeModuleFormat::Json => {
            format!("const value = ({source});\nexport default value;\n")
        }
        HostRuntimeModuleFormat::CommonJs => {
            let escaped_specifier =
                serde_json::to_string(specifier).expect("module specifier should serialize");
            let mut prologue = String::new();
            let mut transformed_source = source.to_string();
            for (index, (request, resolved)) in child_imports.iter().enumerate() {
                let imported_identifier = format!("__cjs_import_{index}");
                let namespace_identifier = format!("__cjs_ns_{index}");
                let resolved_literal =
                    serde_json::to_string(resolved).expect("resolved specifier should serialize");
                prologue.push_str(&format!(
                    "import * as {namespace_identifier} from {resolved_literal};\nconst {imported_identifier} = Object.prototype.hasOwnProperty.call({namespace_identifier}, \"default\") ? {namespace_identifier}.default : {namespace_identifier};\n"
                ));
                for marker in [
                    format!("require(\"{request}\")"),
                    format!("require('{request}')"),
                ] {
                    transformed_source =
                        transformed_source.replace(&marker, imported_identifier.as_str());
                }
            }
            format!(
                r#"const module = {{ exports: {{}} }};
const exports = module.exports;
const __filename = {escaped_specifier};
const __dirname = __filename.includes("/") ? __filename.slice(0, __filename.lastIndexOf("/")) : ".";
const require = (specifier) => {{
  throw new Error(`quickjs-ng CommonJS require is not wired for ${{specifier}} in ${{__filename}}`);
}};
{prologue}{transformed_source}
const __cjs_default = module.exports;
export default __cjs_default;
"#
            )
        }
    }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_ensure_module_registered(
    runtime: &mut QuickJsRuntimeOpaque,
    context_id: &str,
    resolved_specifier: &str,
) -> Result<(), String> {
    if runtime
        .modules_by_context
        .get(context_id)
        .is_some_and(|modules| modules.contains_key(resolved_specifier))
    {
        return Ok(());
    }

    let bridge = runtime
        .bridges_by_context
        .get(context_id)
        .cloned()
        .ok_or_else(|| format!("quickjs-ng runtime bridge missing context {context_id}"))?;
    let file = bridge
        .vfs
        .read(resolved_specifier)
        .cloned()
        .ok_or_else(|| format!("quickjs-ng module file not found: {resolved_specifier}"))?;
    if !file.is_text {
        return Err(format!(
            "quickjs-ng module source must be text: {resolved_specifier}"
        ));
    }

    let format = quickjs_detect_module_format(resolved_specifier);
    let source = String::from_utf8_lossy(&file.bytes).into_owned();
    let dependencies = quickjs_collect_module_dependency_specifiers(&source, &format);
    let mut child_imports = Vec::new();
    for dependency in dependencies {
        let resolved_dependency = quickjs_runtime_resolve_specifier(
            runtime,
            context_id,
            Some(resolved_specifier),
            &dependency,
        )?;
        runtime
            .aliases_by_context
            .entry(context_id.to_string())
            .or_default()
            .insert(
                (resolved_specifier.to_string(), dependency.clone()),
                resolved_dependency.clone(),
            );
        quickjs_ensure_module_registered(runtime, context_id, &resolved_dependency)?;
        child_imports.push((dependency, resolved_dependency));
    }

    let child_import_refs = child_imports
        .iter()
        .map(|(request, resolved)| (request.as_str(), resolved.as_str()))
        .collect::<Vec<_>>();
    let materialized =
        quickjs_materialize_module_source(resolved_specifier, &source, &format, &child_import_refs);
    runtime
        .modules_by_context
        .entry(context_id.to_string())
        .or_default()
        .insert(resolved_specifier.to_string(), materialized);

    Ok(())
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_runtime_resolve_specifier(
    runtime: &QuickJsRuntimeOpaque,
    context_id: &str,
    importer: Option<&str>,
    specifier: &str,
) -> Result<String, String> {
    if runtime
        .modules_by_context
        .get(context_id)
        .is_some_and(|modules| modules.contains_key(specifier))
    {
        return Ok(specifier.to_string());
    }

    let bridge = runtime
        .bridges_by_context
        .get(context_id)
        .ok_or_else(|| format!("quickjs-ng runtime bridge missing context {context_id}"))?;

    if specifier.starts_with("./") || specifier.starts_with("../") || specifier.starts_with('/') {
        let base_dir = importer
            .filter(|value| value.starts_with("/workspace"))
            .map(quickjs_dirname)
            .unwrap_or(&bridge.cwd);
        let requested = if specifier.starts_with('/') {
            normalize_posix_path(specifier)
        } else {
            normalize_posix_path(&format!("{base_dir}/{specifier}"))
        };
        return quickjs_resolve_workspace_module(bridge, &requested);
    }

    let base_dir = importer
        .filter(|value| value.starts_with("/workspace"))
        .map(quickjs_dirname)
        .unwrap_or(&bridge.cwd);
    quickjs_resolve_package_module(bridge, base_dir, specifier)
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_collect_module_dependency_specifiers(
    source: &str,
    format: &HostRuntimeModuleFormat,
) -> Vec<String> {
    let mut specifiers = Vec::new();
    match format {
        HostRuntimeModuleFormat::Module => {
            for marker in [
                " from \"",
                " from '",
                "import(\"",
                "import('",
                "export * from \"",
                "export * from '",
            ] {
                quickjs_collect_string_literals_after_marker(source, marker, &mut specifiers);
            }
            for marker in ["import \"", "import '"] {
                quickjs_collect_line_prefixed_imports(source, marker, &mut specifiers);
            }
        }
        HostRuntimeModuleFormat::CommonJs => {
            for marker in ["require(\"", "require('"] {
                quickjs_collect_string_literals_after_marker(source, marker, &mut specifiers);
            }
        }
        HostRuntimeModuleFormat::Json => {}
    }
    specifiers.sort();
    specifiers.dedup();
    specifiers
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_collect_string_literals_after_marker(
    source: &str,
    marker: &str,
    output: &mut Vec<String>,
) {
    let mut search_start = 0usize;
    while let Some(offset) = source[search_start..].find(marker) {
        let start = search_start + offset + marker.len();
        let quote = marker.as_bytes()[marker.len() - 1] as char;
        if let Some(end_offset) = source[start..].find(quote) {
            let candidate = &source[start..start + end_offset];
            if !candidate.is_empty() {
                output.push(candidate.to_string());
            }
            search_start = start + end_offset + 1;
        } else {
            break;
        }
    }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_collect_line_prefixed_imports(source: &str, marker: &str, output: &mut Vec<String>) {
    for line in source.lines().map(str::trim_start) {
        if let Some(rest) = line.strip_prefix(marker) {
            let quote = marker.as_bytes()[marker.len() - 1] as char;
            if let Some(end) = rest.find(quote) {
                let candidate = &rest[..end];
                if !candidate.is_empty() {
                    output.push(candidate.to_string());
                }
            }
        }
    }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_resolve_workspace_module(
    bridge: &QuickJsRuntimeBridge,
    requested: &str,
) -> Result<String, String> {
    for candidate in quickjs_workspace_module_candidates(requested) {
        if bridge.vfs.read(&candidate).is_some() {
            return Ok(candidate);
        }
    }
    Err(format!("quickjs-ng module not found: {requested}"))
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_resolve_package_module(
    bridge: &QuickJsRuntimeBridge,
    importer_dir: &str,
    specifier: &str,
) -> Result<String, String> {
    let (package_name, subpath) = quickjs_split_package_specifier(specifier);
    for package_root in quickjs_node_module_search_roots(importer_dir, &package_name) {
        let package_json_path = format!("{package_root}/package.json");
        let manifest = bridge
            .vfs
            .read(&package_json_path)
            .and_then(|file| serde_json::from_slice::<JsonValue>(&file.bytes).ok());

        if let Some(subpath) = subpath.as_deref() {
            let requested = normalize_posix_path(&format!("{package_root}/{subpath}"));
            if let Ok(resolved) = quickjs_resolve_workspace_module(bridge, &requested) {
                return Ok(resolved);
            }
        } else if let Some(manifest) = manifest {
            let entry = manifest
                .get("module")
                .and_then(quickjs_json_string)
                .or_else(|| manifest.get("main").and_then(quickjs_json_string));
            if let Some(entry) = entry {
                let requested = normalize_posix_path(&format!("{package_root}/{entry}"));
                if let Ok(resolved) = quickjs_resolve_workspace_module(bridge, &requested) {
                    return Ok(resolved);
                }
            }
        }

        if let Ok(resolved) =
            quickjs_resolve_workspace_module(bridge, &format!("{package_root}/index"))
        {
            return Ok(resolved);
        }
    }

    Err(format!("quickjs-ng module not found: {specifier}"))
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_json_string(value: &JsonValue) -> Option<String> {
    value.as_str().map(ToOwned::to_owned)
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_workspace_module_candidates(requested: &str) -> Vec<String> {
    let normalized = normalize_posix_path(requested);
    [
        normalized.clone(),
        format!("{normalized}.js"),
        format!("{normalized}.mjs"),
        format!("{normalized}.cjs"),
        format!("{normalized}.ts"),
        format!("{normalized}.tsx"),
        format!("{normalized}.jsx"),
        format!("{normalized}.json"),
        format!("{normalized}/index.js"),
        format!("{normalized}/index.mjs"),
        format!("{normalized}/index.cjs"),
        format!("{normalized}/index.ts"),
        format!("{normalized}/index.tsx"),
        format!("{normalized}/index.jsx"),
        format!("{normalized}/index.json"),
    ]
    .into_iter()
    .collect()
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_split_package_specifier(specifier: &str) -> (String, Option<String>) {
    if let Some(stripped) = specifier.strip_prefix('@') {
        let mut parts = stripped.splitn(3, '/');
        let scope = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        let package_name = format!("@{scope}/{name}");
        let subpath = parts.next().map(ToOwned::to_owned);
        return (package_name, subpath);
    }

    let mut parts = specifier.splitn(2, '/');
    let package_name = parts.next().unwrap_or_default().to_string();
    let subpath = parts.next().map(ToOwned::to_owned);
    (package_name, subpath)
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_node_module_search_roots(importer_dir: &str, package_name: &str) -> Vec<String> {
    let mut roots = BTreeMap::new();
    let mut current = importer_dir.to_string();

    while current.starts_with("/workspace") {
        let root = if current.ends_with("/node_modules") {
            normalize_posix_path(&format!("{current}/{package_name}"))
        } else {
            normalize_posix_path(&format!("{current}/node_modules/{package_name}"))
        };
        roots.insert(root.clone(), root);
        if current == "/workspace" {
            break;
        }
        current = quickjs_dirname(&current).to_string();
    }

    roots.into_values().collect()
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_detect_module_format(path: &str) -> HostRuntimeModuleFormat {
    if path.ends_with(".cjs") {
        HostRuntimeModuleFormat::CommonJs
    } else if path.ends_with(".json") {
        HostRuntimeModuleFormat::Json
    } else {
        HostRuntimeModuleFormat::Module
    }
}

#[cfg(feature = "quickjs-ng-engine")]
fn quickjs_dirname(path: &str) -> &str {
    let normalized = path.trim_end_matches('/');
    match normalized.rfind('/') {
        Some(index) if index > 0 => &normalized[..index],
        _ => "/workspace",
    }
}

impl EngineAdapter for NullEngineAdapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            name: "null-engine",
            supports_interrupts: true,
            supports_module_loader: true,
            supports_eval: true,
            supports_job_queue: true,
        }
    }

    fn plan_run(&self, request: &RunRequest) -> RunPlan {
        let command_line = std::iter::once(request.command.as_str())
            .chain(request.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");

        RunPlan {
            cwd: request.cwd.clone(),
            entrypoint: request.command.clone(),
            command_line,
            env_count: request.env.len(),
            command_kind: RunCommandKind::NodeEntrypoint,
            resolved_script: None,
        }
    }

    fn boot_session(&mut self, spec: &EngineSessionSpec) -> Result<EngineSessionHandle, String> {
        Ok(self.state.boot_session(spec, "null-engine-session"))
    }

    fn dispose_session(&mut self, handle: &EngineSessionHandle) {
        self.state.dispose_session(handle);
    }

    fn create_context(&mut self, spec: &EngineContextSpec) -> Result<EngineContextHandle, String> {
        self.state
            .create_context(spec, "null-engine-context", "null engine")
    }

    fn describe_context(&self, handle: &EngineContextHandle) -> Option<EngineContextSnapshot> {
        self.state.describe_context(handle)
    }

    fn eval(
        &mut self,
        handle: &EngineContextHandle,
        request: &EngineEvalRequest,
    ) -> Result<EngineEvalOutcome, String> {
        let mut outcome = self.state.mark_ready(handle, "null engine")?;
        outcome.result_summary = format!(
            "null-engine skipped {:?} eval for {} ({} bytes)",
            request.mode,
            request.filename,
            request.source.len()
        );
        Ok(outcome)
    }

    fn drain_jobs(&mut self, handle: &EngineContextHandle) -> Result<EngineJobDrain, String> {
        self.state.drain_jobs(handle, "null engine")
    }

    fn bootstrap(
        &mut self,
        handle: &EngineContextHandle,
        plan: &HostRuntimeBootstrapPlan,
        loader_plan: &HostRuntimeModuleLoaderPlan,
        import_plans: &[HostRuntimeModuleImportPlan],
        _bridge: &EngineBootstrapBridge,
    ) -> Result<EngineEvalOutcome, String> {
        self.state
            .register_bootstrap(handle, plan, loader_plan, import_plans, "null engine")?;
        let mut outcome = self.state.mark_ready(handle, "null engine")?;
        outcome.result_summary = format!(
            "null-engine prepared bootstrap {} with {} modules across {} loader roots",
            plan.bootstrap_specifier,
            plan.modules.len(),
            loader_plan.node_module_search_roots.len()
        );
        Ok(outcome)
    }

    fn list_modules(
        &self,
        handle: &EngineContextHandle,
    ) -> Result<Vec<EngineRegisteredModule>, String> {
        self.state.list_modules(handle, "null engine")
    }

    fn read_module(
        &self,
        handle: &EngineContextHandle,
        specifier: &str,
    ) -> Result<EngineRegisteredModule, String> {
        self.state.read_module(handle, specifier, "null engine")
    }

    fn take_bridge_snapshot(
        &mut self,
        _handle: &EngineContextHandle,
    ) -> Result<Option<EngineBridgeSnapshot>, String> {
        Ok(None)
    }

    fn fire_timers(
        &mut self,
        _handle: &EngineContextHandle,
        _now_ms: u64,
        _timer_ids: &[String],
    ) -> Result<usize, String> {
        Ok(0)
    }

    fn interrupt(&mut self, handle: &EngineContextHandle, _reason: &str) -> Result<(), String> {
        self.state.interrupt(handle, "null engine")
    }

    fn dispose_context(&mut self, handle: &EngineContextHandle) {
        self.state.dispose_context(handle);
    }
}

impl EngineAdapter for QuickJsNgEngineAdapter {
    fn descriptor(&self) -> EngineDescriptor {
        EngineDescriptor {
            name: {
                #[cfg(feature = "quickjs-ng-engine")]
                {
                    "quickjs-ng-native-bootstrap-loader"
                }
                #[cfg(not(feature = "quickjs-ng-engine"))]
                {
                    "quickjs-ng-stub"
                }
            },
            supports_interrupts: true,
            supports_module_loader: {
                #[cfg(feature = "quickjs-ng-engine")]
                {
                    true
                }
                #[cfg(not(feature = "quickjs-ng-engine"))]
                {
                    false
                }
            },
            supports_eval: true,
            supports_job_queue: true,
        }
    }

    fn plan_run(&self, request: &RunRequest) -> RunPlan {
        let command_line = std::iter::once(request.command.as_str())
            .chain(request.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");

        RunPlan {
            cwd: request.cwd.clone(),
            entrypoint: request.command.clone(),
            command_line,
            env_count: request.env.len(),
            command_kind: RunCommandKind::NodeEntrypoint,
            resolved_script: None,
        }
    }

    fn boot_session(&mut self, spec: &EngineSessionSpec) -> Result<EngineSessionHandle, String> {
        let handle = self.state.boot_session(spec, "quickjs-ng-session");
        #[cfg(feature = "quickjs-ng-engine")]
        self.native.boot_session(&handle)?;
        Ok(handle)
    }

    fn dispose_session(&mut self, handle: &EngineSessionHandle) {
        #[cfg(feature = "quickjs-ng-engine")]
        self.native.dispose_session(handle);
        self.state.dispose_session(handle);
    }

    fn create_context(&mut self, spec: &EngineContextSpec) -> Result<EngineContextHandle, String> {
        let handle = self
            .state
            .create_context(spec, "quickjs-ng-context", "quickjs-ng")?;
        #[cfg(feature = "quickjs-ng-engine")]
        self.native.create_context(&handle)?;
        Ok(handle)
    }

    fn describe_context(&self, handle: &EngineContextHandle) -> Option<EngineContextSnapshot> {
        let snapshot = self.state.describe_context(handle)?;
        #[cfg(feature = "quickjs-ng-engine")]
        {
            let mut snapshot = snapshot;
            if let Ok(modules) = self.native.list_modules(handle) {
                snapshot.registered_modules = modules.len();
            }
            return Some(snapshot);
        }

        #[cfg(not(feature = "quickjs-ng-engine"))]
        Some(snapshot)
    }

    fn eval(
        &mut self,
        handle: &EngineContextHandle,
        _request: &EngineEvalRequest,
    ) -> Result<EngineEvalOutcome, String> {
        #[cfg(feature = "quickjs-ng-engine")]
        {
            self.native.activate_context(handle)?;
            let (result_summary, pending_jobs) = self.native.eval(handle, _request)?;
            let context = self
                .state
                .contexts
                .get_mut(&handle.engine_context_id)
                .ok_or_else(|| {
                    format!("quickjs-ng context not found: {}", handle.engine_context_id)
                })?;
            context.state = EngineContextState::Ready;
            context.pending_jobs = pending_jobs;

            return Ok(EngineEvalOutcome {
                result_summary,
                pending_jobs,
                state: context.state.clone(),
            });
        }

        #[cfg(not(feature = "quickjs-ng-engine"))]
        {
            let snapshot = self.state.describe_context(handle).ok_or_else(|| {
                format!("quickjs-ng context not found: {}", handle.engine_context_id)
            })?;

            Err(format!(
                "quickjs-ng adapter scaffold is ready for {} but the VM crate is not linked yet",
                snapshot.entrypoint
            ))
        }
    }

    fn bootstrap(
        &mut self,
        handle: &EngineContextHandle,
        plan: &HostRuntimeBootstrapPlan,
        loader_plan: &HostRuntimeModuleLoaderPlan,
        import_plans: &[HostRuntimeModuleImportPlan],
        bridge: &EngineBootstrapBridge,
    ) -> Result<EngineEvalOutcome, String> {
        self.state
            .register_bootstrap(handle, plan, loader_plan, import_plans, "quickjs-ng")?;

        #[cfg(not(feature = "quickjs-ng-engine"))]
        let _ = bridge;

        #[cfg(feature = "quickjs-ng-engine")]
        {
            self.native
                .register_bootstrap(handle, plan, import_plans, bridge)?;
            self.native.activate_context(handle)?;
            let bootstrap_source = plan
                .modules
                .iter()
                .find(|module| module.specifier == plan.bootstrap_specifier)
                .map(|module| module.source.clone())
                .ok_or_else(|| {
                    format!(
                        "quickjs-ng bootstrap module not registered: {}",
                        plan.bootstrap_specifier
                    )
                })?;
            let mut outcome = self.eval(
                handle,
                &EngineEvalRequest {
                    filename: plan.bootstrap_specifier.clone(),
                    source: bootstrap_source,
                    mode: EngineEvalMode::Module,
                },
            )?;
            outcome.result_summary = format!(
                "quickjs-ng booted {} bootstrap modules via {} across {} loader roots",
                plan.modules.len(),
                plan.bootstrap_specifier,
                loader_plan.node_module_search_roots.len()
            );
            return Ok(outcome);
        }

        #[cfg(not(feature = "quickjs-ng-engine"))]
        {
            let snapshot = self.state.describe_context(handle).ok_or_else(|| {
                format!("quickjs-ng context not found: {}", handle.engine_context_id)
            })?;

            Err(format!(
                "quickjs-ng adapter scaffold registered {} modules across {} loader roots for {} but the VM crate is not linked yet",
                plan.modules.len(),
                loader_plan.node_module_search_roots.len(),
                snapshot.entrypoint
            ))
        }
    }

    fn drain_jobs(&mut self, handle: &EngineContextHandle) -> Result<EngineJobDrain, String> {
        #[cfg(feature = "quickjs-ng-engine")]
        {
            let drain = self.native.drain_jobs(handle)?;
            let context = self
                .state
                .contexts
                .get_mut(&handle.engine_context_id)
                .ok_or_else(|| {
                    format!("quickjs-ng context not found: {}", handle.engine_context_id)
                })?;
            context.pending_jobs = drain.pending_jobs;
            if context.state != EngineContextState::Disposed {
                context.state = EngineContextState::Ready;
            }
            return Ok(drain);
        }

        #[cfg(not(feature = "quickjs-ng-engine"))]
        {
            self.state.drain_jobs(handle, "quickjs-ng")
        }
    }

    fn list_modules(
        &self,
        handle: &EngineContextHandle,
    ) -> Result<Vec<EngineRegisteredModule>, String> {
        #[cfg(feature = "quickjs-ng-engine")]
        {
            if let Ok(modules) = self.native.list_modules(handle) {
                return Ok(modules);
            }
        }

        #[cfg(not(feature = "quickjs-ng-engine"))]
        {}

        self.state.list_modules(handle, "quickjs-ng")
    }

    fn read_module(
        &self,
        handle: &EngineContextHandle,
        specifier: &str,
    ) -> Result<EngineRegisteredModule, String> {
        #[cfg(feature = "quickjs-ng-engine")]
        {
            if let Ok(module) = self.native.read_module(handle, specifier) {
                return Ok(module);
            }
        }

        #[cfg(not(feature = "quickjs-ng-engine"))]
        {}

        self.state.read_module(handle, specifier, "quickjs-ng")
    }

    fn take_bridge_snapshot(
        &mut self,
        handle: &EngineContextHandle,
    ) -> Result<Option<EngineBridgeSnapshot>, String> {
        #[cfg(feature = "quickjs-ng-engine")]
        {
            return self.native.take_bridge_snapshot(handle);
        }

        #[cfg(not(feature = "quickjs-ng-engine"))]
        {
            let _ = handle;
            Ok(None)
        }
    }

    fn fire_timers(
        &mut self,
        handle: &EngineContextHandle,
        now_ms: u64,
        timer_ids: &[String],
    ) -> Result<usize, String> {
        #[cfg(feature = "quickjs-ng-engine")]
        {
            return self.native.fire_timers(handle, now_ms, timer_ids);
        }

        #[cfg(not(feature = "quickjs-ng-engine"))]
        {
            let _ = (handle, now_ms, timer_ids);
            Ok(0)
        }
    }

    fn interrupt(&mut self, handle: &EngineContextHandle, _reason: &str) -> Result<(), String> {
        #[cfg(feature = "quickjs-ng-engine")]
        {
            return self.native.interrupt(handle);
        }

        #[cfg(not(feature = "quickjs-ng-engine"))]
        {
            self.state.interrupt(handle, "quickjs-ng")
        }
    }

    fn dispose_context(&mut self, handle: &EngineContextHandle) {
        #[cfg(feature = "quickjs-ng-engine")]
        self.native.dispose_context(handle);
        self.state.dispose_context(handle);
    }
}
