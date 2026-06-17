use std::ops::Deref;
use std::path::{Path, PathBuf};

pub struct RelativePathResolver {
    root: PathBuf,
}

impl RelativePathResolver {
    pub fn with_root(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn resolve(&self, path: &Path) -> ResolvedPath {
        let resolved = if path.is_absolute() {
            path.to_owned()
        } else {
            self.root.join(path)
        };

        ResolvedPath { inner: resolved }
    }
}

pub struct ResolvedPath {
    inner: PathBuf,
}

#[cfg(test)]
impl ResolvedPath {
    pub fn from_str(s: &str) -> Self {
        Self {
            inner: PathBuf::from(s),
        }
    }
}

impl Deref for ResolvedPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
