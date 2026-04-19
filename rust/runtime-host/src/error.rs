use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHostError {
    InvalidWorkspacePath(String),
    DuplicateFilePath(String),
    SessionNotFound(String),
    FileNotFound(String),
    ScriptNotFound(String),
    NodeEntrypointRequired,
    UnsupportedCommand(String),
}

impl Display for RuntimeHostError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidWorkspacePath(path) => {
                write!(f, "workspace path must stay under /workspace: {path}")
            }
            Self::DuplicateFilePath(path) => write!(f, "duplicate workspace file path: {path}"),
            Self::SessionNotFound(session_id) => write!(f, "session not found: {session_id}"),
            Self::FileNotFound(path) => write!(f, "workspace file not found: {path}"),
            Self::ScriptNotFound(script) => write!(f, "script not found: {script}"),
            Self::NodeEntrypointRequired => write!(f, "node entrypoint is required"),
            Self::UnsupportedCommand(command) => write!(f, "unsupported command: {command}"),
        }
    }
}

impl std::error::Error for RuntimeHostError {}

pub type RuntimeHostResult<T> = Result<T, RuntimeHostError>;
