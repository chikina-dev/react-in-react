use std::collections::{BTreeMap, BTreeSet};

use crate::error::{RuntimeHostError, RuntimeHostResult};
use crate::protocol::{WorkspaceEntryKind, WorkspaceEntrySummary};

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
    directories: BTreeSet<String>,
}

impl VirtualFileSystem {
    pub fn new(workspace_root: impl Into<String>) -> Self {
        Self {
            workspace_root: normalize_posix_path(&workspace_root.into()),
            files: BTreeMap::new(),
            directories: BTreeSet::new(),
        }
    }

    pub fn workspace_root(&self) -> &str {
        &self.workspace_root
    }

    pub fn mount_files(
        &mut self,
        files: impl IntoIterator<Item = VirtualFile>,
    ) -> RuntimeHostResult<()> {
        self.directories.insert(self.workspace_root.clone());

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

            for directory in parent_directories(&normalized, &self.workspace_root) {
                self.directories.insert(directory);
            }
        }

        Ok(())
    }

    pub fn read(&self, path: &str) -> Option<&VirtualFile> {
        self.files.get(path)
    }

    pub fn create_dir_all(&mut self, path: &str) -> RuntimeHostResult<()> {
        let normalized = normalize_posix_path(path);

        if !normalized.starts_with(&self.workspace_root) {
            return Err(RuntimeHostError::InvalidWorkspacePath(normalized));
        }

        if self.files.contains_key(&normalized) {
            return Err(RuntimeHostError::NotADirectory(normalized));
        }

        for directory in parent_directories(&normalized, &self.workspace_root) {
            self.directories.insert(directory);
        }

        self.directories.insert(normalized);
        Ok(())
    }

    pub fn write_file(
        &mut self,
        path: &str,
        bytes: Vec<u8>,
        is_text: bool,
    ) -> RuntimeHostResult<WorkspaceEntrySummary> {
        let normalized = normalize_posix_path(path);

        if !normalized.starts_with(&self.workspace_root) {
            return Err(RuntimeHostError::InvalidWorkspacePath(normalized));
        }

        if self.directories.contains(&normalized) {
            return Err(RuntimeHostError::IsADirectory(normalized));
        }

        for directory in parent_directories(&normalized, &self.workspace_root) {
            self.directories.insert(directory);
        }

        let size = bytes.len();
        self.files.insert(
            normalized.clone(),
            VirtualFile {
                path: normalized.clone(),
                bytes,
                is_text,
            },
        );

        Ok(WorkspaceEntrySummary {
            path: normalized,
            kind: WorkspaceEntryKind::File,
            size,
            is_text,
        })
    }

    pub fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path) || self.directories.contains(path)
    }

    pub fn is_dir(&self, path: &str) -> bool {
        self.directories.contains(path)
    }

    pub fn stat(&self, path: &str) -> Option<WorkspaceEntrySummary> {
        if let Some(file) = self.files.get(path) {
            return Some(WorkspaceEntrySummary {
                path: file.path.clone(),
                kind: WorkspaceEntryKind::File,
                size: file.bytes.len(),
                is_text: file.is_text,
            });
        }

        self.directories
            .get(path)
            .map(|directory| WorkspaceEntrySummary {
                path: directory.clone(),
                kind: WorkspaceEntryKind::Directory,
                size: 0,
                is_text: false,
            })
    }

    pub fn read_dir(&self, path: &str) -> RuntimeHostResult<Vec<WorkspaceEntrySummary>> {
        if !self.exists(path) {
            return Err(RuntimeHostError::DirectoryNotFound(path.into()));
        }

        if !self.is_dir(path) {
            return Err(RuntimeHostError::NotADirectory(path.into()));
        }

        let mut entries = BTreeMap::new();

        for directory in &self.directories {
            if directory == path {
                continue;
            }

            if parent_path(directory) == path {
                entries.insert(
                    directory.clone(),
                    WorkspaceEntrySummary {
                        path: directory.clone(),
                        kind: WorkspaceEntryKind::Directory,
                        size: 0,
                        is_text: false,
                    },
                );
            }
        }

        for file in self.files.values() {
            if parent_path(&file.path) == path {
                entries.insert(
                    file.path.clone(),
                    WorkspaceEntrySummary {
                        path: file.path.clone(),
                        kind: WorkspaceEntryKind::File,
                        size: file.bytes.len(),
                        is_text: file.is_text,
                    },
                );
            }
        }

        Ok(entries.into_values().collect())
    }

    pub fn files(&self) -> impl Iterator<Item = &VirtualFile> {
        self.files.values()
    }

    pub fn directories(&self) -> impl Iterator<Item = &String> {
        self.directories.iter()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn directory_count(&self) -> usize {
        self.directories.len()
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

fn parent_directories(path: &str, workspace_root: &str) -> Vec<String> {
    let mut directories = Vec::new();
    let mut current = parent_path(path).to_string();

    while current.starts_with(workspace_root) {
        directories.push(current.clone());

        if current == workspace_root {
            break;
        }

        current = parent_path(&current).to_string();
    }

    directories
}

fn parent_path(path: &str) -> &str {
    let normalized = path.trim_end_matches('/');

    if normalized.is_empty() || normalized == "/" {
        return "/";
    }

    normalized
        .rsplit_once('/')
        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
        .unwrap_or("/")
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

    #[test]
    fn mount_files_registers_parent_directories_and_reads_directories() {
        let mut vfs = VirtualFileSystem::new("/workspace");
        vfs.mount_files([
            VirtualFile::text("/workspace/package.json", "{}"),
            VirtualFile::text("/workspace/src/main.tsx", "export default null;"),
            VirtualFile::binary("/workspace/public/logo.png", vec![0x89, 0x50]),
        ])
        .expect("mount should succeed");

        assert!(vfs.is_dir("/workspace"));
        assert!(vfs.is_dir("/workspace/src"));
        assert!(vfs.is_dir("/workspace/public"));
        assert_eq!(vfs.directory_count(), 3);
        assert_eq!(
            vfs.stat("/workspace/src")
                .expect("directory stat should exist")
                .kind,
            WorkspaceEntryKind::Directory
        );

        assert_eq!(
            vfs.read_dir("/workspace")
                .expect("workspace directory should list entries")
                .into_iter()
                .map(|entry| entry.path)
                .collect::<Vec<_>>(),
            vec![
                "/workspace/package.json".to_string(),
                "/workspace/public".to_string(),
                "/workspace/src".to_string(),
            ]
        );
    }

    #[test]
    fn create_dir_all_and_write_file_mutate_workspace_tree() {
        let mut vfs = VirtualFileSystem::new("/workspace");
        vfs.mount_files([VirtualFile::text("/workspace/package.json", "{}")])
            .expect("mount should succeed");

        vfs.create_dir_all("/workspace/src/generated")
            .expect("directory creation should succeed");
        let written = vfs
            .write_file(
                "/workspace/src/generated/app.js",
                b"console.log('generated');".to_vec(),
                true,
            )
            .expect("file write should succeed");

        assert!(vfs.is_dir("/workspace/src"));
        assert!(vfs.is_dir("/workspace/src/generated"));
        assert_eq!(written.path, "/workspace/src/generated/app.js");
        assert_eq!(written.size, 25);
        assert_eq!(
            vfs.read("/workspace/src/generated/app.js")
                .expect("generated file should exist")
                .bytes,
            b"console.log('generated');"
        );
    }
}
