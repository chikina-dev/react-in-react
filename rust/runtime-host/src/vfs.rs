use std::collections::BTreeMap;

use crate::error::{RuntimeHostError, RuntimeHostResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VirtualFile {
    pub path: String,
    pub bytes: Vec<u8>,
    pub is_text: bool,
}

impl VirtualFile {
    pub fn text(path: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            bytes: content.into().into_bytes(),
            is_text: true,
        }
    }

    pub fn binary(path: impl Into<String>, bytes: Vec<u8>) -> Self {
        Self {
            path: path.into(),
            bytes,
            is_text: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct VirtualFileSystem {
    workspace_root: String,
    files: BTreeMap<String, VirtualFile>,
}

impl VirtualFileSystem {
    pub fn new(workspace_root: impl Into<String>) -> Self {
        Self {
            workspace_root: normalize_posix_path(&workspace_root.into()),
            files: BTreeMap::new(),
        }
    }

    pub fn workspace_root(&self) -> &str {
        &self.workspace_root
    }

    pub fn mount_files(
        &mut self,
        files: impl IntoIterator<Item = VirtualFile>,
    ) -> RuntimeHostResult<()> {
        for file in files {
            let normalized = normalize_posix_path(&file.path);

            if !normalized.starts_with(&self.workspace_root) {
                return Err(RuntimeHostError::InvalidWorkspacePath(normalized));
            }

            if self
                .files
                .insert(
                    normalized.clone(),
                    VirtualFile {
                        path: normalized.clone(),
                        ..file
                    },
                )
                .is_some()
            {
                return Err(RuntimeHostError::DuplicateFilePath(normalized));
            }
        }

        Ok(())
    }

    pub fn read(&self, path: &str) -> Option<&VirtualFile> {
        self.files.get(path)
    }

    pub fn files(&self) -> impl Iterator<Item = &VirtualFile> {
        self.files.values()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

pub fn normalize_posix_path(input: &str) -> String {
    let is_absolute = input.starts_with('/');
    let mut parts = Vec::new();

    for segment in input.split('/').filter(|segment| !segment.is_empty()) {
        match segment {
            "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(segment),
        }
    }

    if parts.is_empty() {
        return if is_absolute { "/".into() } else { ".".into() };
    }

    if is_absolute {
        format!("/{}", parts.join("/"))
    } else {
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_posix_path_collapses_relative_segments() {
        assert_eq!(
            normalize_posix_path("/workspace/src/../index.ts"),
            "/workspace/index.ts"
        );
        assert_eq!(
            normalize_posix_path("/workspace//nested/./file.js"),
            "/workspace/nested/file.js"
        );
    }

    #[test]
    fn mount_files_rejects_outside_workspace() {
        let mut vfs = VirtualFileSystem::new("/workspace");
        let error = vfs
            .mount_files([VirtualFile::text("/tmp/leak.txt", "nope")])
            .expect_err("file outside workspace should fail");

        assert!(
            matches!(error, RuntimeHostError::InvalidWorkspacePath(path) if path == "/tmp/leak.txt")
        );
    }
}
