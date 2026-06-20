use std::fmt::{Display, Formatter};
use std::str::FromStr;

/// Represents an app name on the server
///
/// App names are mapped to Linux users, and this type enforces that the string it carries is a
/// valid username.
#[derive(Clone)]
pub struct AppName {
    inner: String,
}

impl AppName {
    pub fn as_str(&self) -> &str {
        &self.inner
    }
}

impl Display for AppName {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl FromStr for AppName {
    type Err = &'static str;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut chars = s.chars();
        if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) {
            return Err("the name should start with an alphabetic ASCII character");
        }

        let valid = chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !valid {
            return Err(
                "only alphanumeric ASCII characters, dashes (`-`), and underscores (`_`) are allowed",
            );
        }

        Ok(Self {
            inner: s.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty() {
        assert!(AppName::from_str("").is_err());
    }

    #[test]
    fn test_valid() {
        assert!(AppName::from_str("foo").is_ok());
        assert!(AppName::from_str("foo-bar").is_ok());
        assert!(AppName::from_str("foo42_bar").is_ok());
    }

    #[test]
    fn test_invalid() {
        // Non-alphabetic start
        assert!(AppName::from_str("-foo").is_err());
        assert!(AppName::from_str("42foo").is_err());
        assert!(AppName::from_str("_foo").is_err());

        // Disallowed chars
        assert!(AppName::from_str("foo:bar").is_err());
        assert!(AppName::from_str("foo/bar").is_err());
        assert!(AppName::from_str("foo?bar").is_err());
    }
}
