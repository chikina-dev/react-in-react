use std::collections::BTreeMap;
#[cfg(feature = "quickjs-ng-engine")]
use std::ffi::{CStr, CString};
#[cfg(feature = "quickjs-ng-engine")]
use std::ptr;

use crate::protocol::{HostRuntimeBootstrapPlan, HostRuntimeModuleLoaderPlan, RunCommandKind, RunPlan, RunRequest};
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

        context.registered_modules = plan.modules.len();
        context.bootstrap_specifier = Some(plan.bootstrap_specifier.clone());
        context.module_loader_roots = loader_plan.node_module_search_roots.clone();
        context.modules = plan
            .modules
            .iter()
            .map(|module| (module.specifier.clone(), module.source.clone()))
            .collect();

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
            .ok_or_else(|| format!("{engine_label} context not found: {}", handle.engine_context_id))?;

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
            .ok_or_else(|| format!("{engine_label} context not found: {}", handle.engine_context_id))?;

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

#[derive(Debug, Default)]
pub struct QuickJsNgEngineAdapter {
    state: EngineStateStore,
    #[cfg(feature = "quickjs-ng-engine")]
    native: QuickJsNativeStore,
}

#[cfg(feature = "quickjs-ng-engine")]
#[derive(Debug)]
struct QuickJsNativeSessionRecord {
    runtime: *mut qjs::JSRuntime,
}

#[cfg(feature = "quickjs-ng-engine")]
#[derive(Debug)]
struct QuickJsNativeContextRecord {
    engine_session_id: String,
    runtime: *mut qjs::JSRuntime,
    context: *mut qjs::JSContext,
}

#[cfg(feature = "quickjs-ng-engine")]
#[derive(Debug, Default)]
struct QuickJsNativeStore {
    sessions: BTreeMap<String, QuickJsNativeSessionRecord>,
    contexts: BTreeMap<String, QuickJsNativeContextRecord>,
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

        self.sessions.insert(
            handle.engine_session_id.clone(),
            QuickJsNativeSessionRecord { runtime },
        );
        Ok(())
    }

    fn create_context(&mut self, handle: &EngineContextHandle) -> Result<(), String> {
        let session = self
            .sessions
            .get(&handle.engine_session_id)
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

    fn eval(
        &mut self,
        handle: &EngineContextHandle,
        request: &EngineEvalRequest,
    ) -> Result<(String, usize), String> {
        let context = self
            .contexts
            .get(&handle.engine_context_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng context not found: {}",
                    handle.engine_context_id
                )
            })?;

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
            EngineEvalMode::Script => {
                (qjs::JS_EVAL_TYPE_GLOBAL | qjs::JS_EVAL_FLAG_STRICT) as i32
            }
            EngineEvalMode::Module => {
                (qjs::JS_EVAL_TYPE_MODULE | qjs::JS_EVAL_FLAG_STRICT) as i32
            }
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

    fn drain_jobs(&mut self, handle: &EngineContextHandle) -> Result<EngineJobDrain, String> {
        let context = self
            .contexts
            .get(&handle.engine_context_id)
            .ok_or_else(|| {
                format!(
                    "quickjs-ng context not found: {}",
                    handle.engine_context_id
                )
            })?;

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
    ) -> Result<EngineEvalOutcome, String> {
        self.state
            .register_bootstrap(handle, plan, loader_plan, "null engine")?;
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
                    "quickjs-ng-native-loader-stub"
                }
                #[cfg(not(feature = "quickjs-ng-engine"))]
                {
                    "quickjs-ng-stub"
                }
            },
            supports_interrupts: true,
            supports_module_loader: false,
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
            .create_context(spec, "quickjs-ng-context", "quickjs-ng")
            ?;
        #[cfg(feature = "quickjs-ng-engine")]
        self.native.create_context(&handle)?;
        Ok(handle)
    }

    fn describe_context(&self, handle: &EngineContextHandle) -> Option<EngineContextSnapshot> {
        self.state.describe_context(handle)
    }

    fn eval(
        &mut self,
        handle: &EngineContextHandle,
        _request: &EngineEvalRequest,
    ) -> Result<EngineEvalOutcome, String> {
        #[cfg(feature = "quickjs-ng-engine")]
        {
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
        let snapshot = self
            .state
            .describe_context(handle)
            .ok_or_else(|| format!("quickjs-ng context not found: {}", handle.engine_context_id))?;

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
    ) -> Result<EngineEvalOutcome, String> {
        self.state
            .register_bootstrap(handle, plan, loader_plan, "quickjs-ng")?;

        #[cfg(feature = "quickjs-ng-engine")]
        {
            let snapshot = self
                .state
                .describe_context(handle)
                .ok_or_else(|| format!("quickjs-ng context not found: {}", handle.engine_context_id))?;

            return Err(format!(
                "quickjs-ng runtime is linked for {} and registered {} modules across {} loader roots, but the module loader callback is not wired yet",
                snapshot.entrypoint,
                plan.modules.len(),
                loader_plan.node_module_search_roots.len()
            ));
        }

        #[cfg(not(feature = "quickjs-ng-engine"))]
        {
        let snapshot = self
            .state
            .describe_context(handle)
            .ok_or_else(|| format!("quickjs-ng context not found: {}", handle.engine_context_id))?;

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
        self.state.list_modules(handle, "quickjs-ng")
    }

    fn read_module(
        &self,
        handle: &EngineContextHandle,
        specifier: &str,
    ) -> Result<EngineRegisteredModule, String> {
        self.state.read_module(handle, specifier, "quickjs-ng")
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
