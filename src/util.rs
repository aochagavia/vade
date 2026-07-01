use miette::{LabeledSpan, Report, SourceCode, miette};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use toml_span::Span;

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

pub fn labeled_span(message: String, span: Span) -> LabeledSpan {
    LabeledSpan::new_with_span(Some(message), span.start..span.end)
}

pub fn diagnostic<S: SourceCode + 'static>(
    error: &str,
    details: String,
    span: Span,
    source: S,
) -> Report {
    let labels = vec![labeled_span(details, span)];
    miette!(labels = labels, "{error}").with_source_code(source)
}

pub fn diagnostic_with_help<S: SourceCode + 'static>(
    error: &str,
    details: String,
    help: String,
    span: Span,
    source: S,
) -> Report {
    let labels = vec![labeled_span(details, span)];
    miette!(labels = labels, help = help, "{error}").with_source_code(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path_resolver(root: &str, path: &str, expected: &str) {
        let resolver = RelativePathResolver::with_root(PathBuf::from(root));
        let resolved = resolver.resolve(Path::new(path));
        assert_eq!(&*resolved, Path::new(expected));
    }

    #[test]
    fn test_path_resolver_absolute() {
        test_path_resolver("/home/johndoe", "/opt/some-path", "/opt/some-path");
    }

    #[test]
    fn test_path_resolver_relative() {
        test_path_resolver("/home/johndoe", "some-path", "/home/johndoe/some-path");
    }
}
