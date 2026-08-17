//! Tiling config: the values that control the layout of panes. Like
//! the theme, it is a set of named values loaded from disk; the first
//! is the gap.

use std::path::Path;

use serde::{Deserialize, Serialize};

/// The values that control the tiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tiling {
    /// The margin each pane keeps from its neighbors and the canvas
    /// edge, in cells.
    pub gap: u16,
}

impl Default for Tiling {
    fn default() -> Self {
        Tiling { gap: 1 }
    }
}

impl Tiling {
    /// Load from `<root>/tiling.json`; the default when the file is
    /// absent or unreadable.
    pub fn load(root: &Path) -> Tiling {
        std::fs::read_to_string(root.join("tiling.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_from_disk_or_defaults() {
        let dir = std::env::temp_dir().join(format!("anvil-tiling-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        assert_eq!(Tiling::load(&dir), Tiling::default());

        std::fs::write(dir.join("tiling.json"), r#"{"gap": 4}"#).unwrap();
        assert_eq!(Tiling::load(&dir), Tiling { gap: 4 });

        let _ = std::fs::remove_dir_all(&dir);
    }
}
