use std::collections::BTreeMap;

use crate::protocol::{RunCommandKind, RunPlan, RunRequest};

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
            state: context.state.clone(),
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
            .ok_or_else(|| format!("{engine_label} context not found: {}", handle.engine_context_id))?;

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
            .ok_or_else(|| format!("{engine_label} context not found: {}", handle.engine_context_id))?;
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
            name: "quickjs-ng-stub",
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
        Ok(self.state.boot_session(spec, "quickjs-ng-session"))
    }

    fn dispose_session(&mut self, handle: &EngineSessionHandle) {
        self.state.dispose_session(handle);
    }

    fn create_context(&mut self, spec: &EngineContextSpec) -> Result<EngineContextHandle, String> {
        self.state
            .create_context(spec, "quickjs-ng-context", "quickjs-ng")
    }

    fn describe_context(&self, handle: &EngineContextHandle) -> Option<EngineContextSnapshot> {
        self.state.describe_context(handle)
    }

    fn eval(
        &mut self,
        handle: &EngineContextHandle,
        _request: &EngineEvalRequest,
    ) -> Result<EngineEvalOutcome, String> {
        let snapshot = self
            .state
            .describe_context(handle)
            .ok_or_else(|| format!("quickjs-ng context not found: {}", handle.engine_context_id))?;

        Err(format!(
            "quickjs-ng adapter scaffold is ready for {} but the VM crate is not linked yet",
            snapshot.entrypoint
        ))
    }

    fn drain_jobs(&mut self, handle: &EngineContextHandle) -> Result<EngineJobDrain, String> {
        self.state.drain_jobs(handle, "quickjs-ng")
    }

    fn interrupt(&mut self, handle: &EngineContextHandle, _reason: &str) -> Result<(), String> {
        self.state.interrupt(handle, "quickjs-ng")
    }

    fn dispose_context(&mut self, handle: &EngineContextHandle) {
        self.state.dispose_context(handle);
    }
}
