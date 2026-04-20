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
    RuntimeContextNotFound(String),
    FileNotFound(String),
    PortAlreadyInUse(u16),
    PortNotListening(u16),
    HttpServerNotFound(u16),
    EngineFailure(String),
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
            Self::RuntimeContextNotFound(context_id) => {
                write!(f, "runtime context not found: {context_id}")
            }
            Self::FileNotFound(path) => write!(f, "workspace file not found: {path}"),
            Self::PortAlreadyInUse(port) => write!(f, "runtime port already in use: {port}"),
            Self::PortNotListening(port) => write!(f, "runtime port not listening: {port}"),
            Self::HttpServerNotFound(port) => write!(f, "runtime http server not found: {port}"),
            Self::EngineFailure(message) => write!(f, "runtime engine failure: {message}"),
            Self::ScriptNotFound(script) => write!(f, "script not found: {script}"),
            Self::NodeEntrypointRequired => write!(f, "node entrypoint is required"),
            Self::EntrypointNotFound(path) => write!(f, "entrypoint not found: {path}"),
            Self::UnsupportedCommand(command) => write!(f, "unsupported command: {command}"),
        }
    }
}

impl std::error::Error for RuntimeHostError {}

pub type RuntimeHostResult<T> = Result<T, RuntimeHostError>;
