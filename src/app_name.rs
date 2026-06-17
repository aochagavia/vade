use std::fmt::{Display, Formatter};
use std::str::FromStr;

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
        if !s.is_ascii() {
            return Err("only ASCII is allowed inside application names");
        }

        let valid = s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !valid {
            return Err(
                "only alphanumeric characters, dashes (`-`), and underscores (`_`) are allowed",
            );
        }

        Ok(Self {
            inner: s.to_string(),
        })
    }
}
