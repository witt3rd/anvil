//! Root seat. `Smith` is the current app. `Window` is the parallel
//! experience. Same binary; the switch is the launch.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Experience {
    #[default]
    Smith,
    Window,
}

impl Experience {
    pub fn parse(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "smith" => Ok(Self::Smith),
            "window" | "casing" => Ok(Self::Window),
            other => Err(format!(
                "unknown experience '{other}': smith or window"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Smith => "smith",
            Self::Window => "window",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_the_two_roots() {
        assert_eq!(Experience::parse("").unwrap(), Experience::Smith);
        assert_eq!(Experience::parse("smith").unwrap(), Experience::Smith);
        assert_eq!(Experience::parse("window").unwrap(), Experience::Window);
        assert_eq!(Experience::parse("casing").unwrap(), Experience::Window);
        assert!(Experience::parse("herdr").is_err());
    }
}
