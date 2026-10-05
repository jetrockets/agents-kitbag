//! A token on its way between a form, a credential store and a request.

/// A secret that never prints: `Debug` and logs show a placeholder.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself, for the one place that sends or stores it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(..)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_secret_never_prints() {
        let secret = Secret::new("figd_abc");
        assert_eq!(format!("{secret:?}"), "Secret(..)");
        assert_eq!(secret.expose(), "figd_abc");
    }
}
