use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeHostError {
    InvalidWorkspacePath(String),
    InvalidWorkingDirectory(String),
    DirectoryNotFound(String),
    NotADirectory(String),
    IsADirectory(String),
    DuplicateFilePath(String),
    SessionNotFound(String),
    FileNotFound(String),
    ScriptNotFound(String),
    NodeEntrypointRequired,
    EntrypointNotFound(String),
    UnsupportedCommand(String),
}

impl Display for RuntimeHostError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidWorkspacePath(path) => {
                write!(f, "workspace path must stay under /workspace: {path}")
            }
            Self::InvalidWorkingDirectory(path) => {
                write!(f, "working directory must stay under /workspace: {path}")
            }
            Self::DirectoryNotFound(path) => write!(f, "workspace directory not found: {path}"),
            Self::NotADirectory(path) => write!(f, "workspace path is not a directory: {path}"),
            Self::IsADirectory(path) => write!(f, "workspace path is a directory: {path}"),
            Self::DuplicateFilePath(path) => write!(f, "duplicate workspace file path: {path}"),
            Self::SessionNotFound(session_id) => write!(f, "session not found: {session_id}"),
            Self::FileNotFound(path) => write!(f, "workspace file not found: {path}"),
            Self::ScriptNotFound(script) => write!(f, "script not found: {script}"),
            Self::NodeEntrypointRequired => write!(f, "node entrypoint is required"),
            Self::EntrypointNotFound(path) => write!(f, "entrypoint not found: {path}"),
            Self::UnsupportedCommand(command) => write!(f, "unsupported command: {command}"),
        }
    }
}

impl std::error::Error for RuntimeHostError {}

pub type RuntimeHostResult<T> = Result<T, RuntimeHostError>;
