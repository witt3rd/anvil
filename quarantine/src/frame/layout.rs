use std::collections::{BTreeMap, HashSet};
use std::fs;

use serde::{Deserialize, Serialize};

use super::{FrameError, FrameRoot};

/// Saved arrangement of a catalog: what's front, and pane tiles.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Layout {
    pub name: String,
    pub catalog: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front_workspace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub front_session: Option<String>,
    /// Relative heights of stage members (workspace order). Empty = equal.
    /// Used when a workspace has no tile tree yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub weights: Vec<u16>,
    /// Per-workspace split tree. Missing = stack every stage member.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tiles: BTreeMap<String, Tile>,
}

impl Layout {
    pub fn for_catalog(name: impl Into<String>, catalog: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            catalog: catalog.into(),
            front_workspace: Some("default".into()),
            front_session: Some("default".into()),
            weights: Vec::new(),
            tiles: BTreeMap::new(),
        }
    }
}

/// Herdr names vs ratatui: `Row` is prefix+v (vertical bar, panes side by side).
/// `Col` is prefix+- (horizontal bar, panes stacked).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SplitDir {
    Row,
    Col,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum Tile {
    Leaf(String),
    Split {
        dir: SplitDir,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        weights: Vec<u16>,
        kids: Vec<Tile>,
    },
}

impl Tile {
    pub fn from_stage(ids: &[String], weights: &[u16]) -> Option<Self> {
        match ids {
            [] => None,
            [id] => Some(Tile::Leaf(id.clone())),
            _ => {
                let weights = if weights.len() == ids.len() {
                    weights.to_vec()
                } else {
                    vec![1; ids.len()]
                };
                Some(Tile::Split {
                    dir: SplitDir::Col,
                    weights,
                    kids: ids.iter().cloned().map(Tile::Leaf).collect(),
                })
            }
        }
    }

    pub fn leaves(&self) -> Vec<&str> {
        match self {
            Tile::Leaf(id) => vec![id],
            Tile::Split { kids, .. } => kids.iter().flat_map(Tile::leaves).collect(),
        }
    }

    /// Bisect the focused leaf. The new pane sits to the right (`Row`) or below (`Col`).
    pub fn split(&mut self, focus: &str, new_id: &str, dir: SplitDir) -> bool {
        match self {
            Tile::Leaf(id) if id == focus => {
                let old = id.clone();
                *self = Tile::Split {
                    dir,
                    weights: vec![1, 1],
                    kids: vec![Tile::Leaf(old), Tile::Leaf(new_id.to_string())],
                };
                true
            }
            Tile::Leaf(_) => false,
            Tile::Split { kids, .. } => kids.iter_mut().any(|k| k.split(focus, new_id, dir)),
        }
    }

    pub fn sync_stage(&mut self, stage: &[String]) {
        let have: HashSet<&str> = self.leaves().into_iter().collect();
        let missing: Vec<String> = stage
            .iter()
            .filter(|id| !have.contains(id.as_str()))
            .cloned()
            .collect();
        if missing.is_empty() {
            return;
        }
        let extras: Vec<Tile> = missing.into_iter().map(Tile::Leaf).collect();
        match self {
            Tile::Split {
                dir: SplitDir::Col,
                kids,
                weights,
            } => {
                kids.extend(extras);
                while weights.len() < kids.len() {
                    weights.push(1);
                }
            }
            other => {
                let old = other.clone();
                *other = Tile::Split {
                    dir: SplitDir::Col,
                    weights: vec![1, 1],
                    kids: std::iter::once(old).chain(extras).collect(),
                };
            }
        }
    }

    /// Drop leaves not in `keep`. Flatten unary splits. False = this node is gone.
    pub fn prune(&mut self, keep: &HashSet<String>) -> bool {
        match self {
            Tile::Leaf(id) => keep.contains(id),
            Tile::Split { dir, kids, weights } => {
                let dir = *dir;
                let mut next = Vec::new();
                let mut next_w = Vec::new();
                for (i, mut kid) in kids.drain(..).enumerate() {
                    if kid.prune(keep) {
                        next.push(kid);
                        next_w.push(weights.get(i).copied().unwrap_or(1));
                    }
                }
                match next.len() {
                    0 => false,
                    1 => {
                        *self = next.pop().unwrap();
                        true
                    }
                    _ => {
                        *self = Tile::Split {
                            dir,
                            weights: next_w,
                            kids: next,
                        };
                        true
                    }
                }
            }
        }
    }

    pub fn bump_weight(&mut self, focus: &str, delta: i16) -> bool {
        match self {
            Tile::Leaf(_) => false,
            Tile::Split { kids, weights, .. } => {
                if weights.len() != kids.len() {
                    *weights = vec![1; kids.len()];
                }
                for (i, kid) in kids.iter().enumerate() {
                    if matches!(kid, Tile::Leaf(id) if id == focus) {
                        weights[i] = clamp_weight(weights[i], delta);
                        return true;
                    }
                }
                kids.iter_mut().any(|k| k.bump_weight(focus, delta))
            }
        }
    }

    pub fn at_mut(&mut self, path: &[usize]) -> Option<&mut Tile> {
        let mut cur = self;
        for &i in path {
            match cur {
                Tile::Split { kids, .. } => cur = kids.get_mut(i)?,
                Tile::Leaf(_) => return None,
            }
        }
        Some(cur)
    }

    pub fn seed_weights(&mut self, sizes: &[u16]) -> bool {
        match self {
            Tile::Split { kids, weights, .. } if kids.len() == sizes.len() => {
                *weights = sizes.iter().map(|s| (*s).max(1)).collect();
                true
            }
            _ => false,
        }
    }

    pub fn set_gap(&mut self, gap: usize, px_a: u16, px_b: u16, delta: i32) -> bool {
        match self {
            Tile::Split { weights, .. } => apply_gap(weights, gap, px_a, px_b, delta, 3),
            Tile::Leaf(_) => false,
        }
    }

    pub fn equalize(&mut self) -> bool {
        match self {
            Tile::Split { kids, weights, .. } => {
                *weights = vec![1; kids.len()];
                true
            }
            Tile::Leaf(_) => false,
        }
    }

    pub fn swap_ids(&mut self, a: &str, b: &str) {
        match self {
            Tile::Leaf(id) if id == a => *id = b.to_string(),
            Tile::Leaf(id) if id == b => *id = a.to_string(),
            Tile::Split { kids, .. } => {
                for kid in kids {
                    kid.swap_ids(a, b);
                }
            }
            Tile::Leaf(_) => {}
        }
    }

    pub fn rename_id(&mut self, old: &str, new: &str) {
        match self {
            Tile::Leaf(id) if id == old => *id = new.to_string(),
            Tile::Split { kids, .. } => {
                for kid in kids {
                    kid.rename_id(old, new);
                }
            }
            Tile::Leaf(_) => {}
        }
    }
}

pub const WEIGHT_MIN: u16 = 1;
pub const WEIGHT_MAX: u16 = 512;

pub fn clamp_weight(weight: u16, delta: i16) -> u16 {
    (i32::from(weight) + i32::from(delta)).clamp(i32::from(WEIGHT_MIN), i32::from(WEIGHT_MAX)) as u16
}

/// Keep a pair's combined size; grow the first kid by `delta` cells.
pub fn clamp_pair(px_a: u16, px_b: u16, delta: i32, min: u16) -> (u16, u16) {
    let min = i32::from(min.max(1));
    let a = i32::from(px_a);
    let b = i32::from(px_b);
    let mut da = delta;
    if a + da < min {
        da = min - a;
    }
    if b - da < min {
        da = b - min;
    }
    ((a + da) as u16, (b - da) as u16)
}

pub fn apply_gap(
    weights: &mut [u16],
    gap: usize,
    px_a: u16,
    px_b: u16,
    delta: i32,
    min: u16,
) -> bool {
    if gap + 1 >= weights.len() {
        return false;
    }
    let (na, nb) = clamp_pair(px_a, px_b, delta, min);
    weights[gap] = na.max(1);
    weights[gap + 1] = nb.max(1);
    true
}

impl FrameRoot {
    fn layout_path(&self, name: &str) -> std::path::PathBuf {
        self.root.join("layouts").join(format!("{name}.json"))
    }

    pub fn layout_exists(&self, name: &str) -> bool {
        FrameRoot::parse_name(name)
            .map(|n| self.layout_path(&n).is_file())
            .unwrap_or(false)
    }

    pub fn layout(&self, name: &str) -> Result<Layout, FrameError> {
        let name = Self::parse_name(name)?;
        let path = self.layout_path(&name);
        if !path.is_file() {
            return Err(FrameError::UnknownLayout(name));
        }
        self.read_json(&path)
    }

    pub fn save_layout(&self, layout: &Layout) -> Result<(), FrameError> {
        let name = Self::parse_name(&layout.name)?;
        self.write_json(&self.layout_path(&name), layout)
    }

    pub fn delete_layout(&self, name: &str) -> Result<(), FrameError> {
        let name = Self::parse_name(name)?;
        let path = self.layout_path(&name);
        if !path.is_file() {
            return Err(FrameError::UnknownLayout(name));
        }
        fs::remove_file(path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_bisects_the_focused_leaf() {
        let mut t = Tile::Leaf("audit".into());
        assert!(t.split("audit", "bash", SplitDir::Row));
        assert_eq!(
            t,
            Tile::Split {
                dir: SplitDir::Row,
                weights: vec![1, 1],
                kids: vec![Tile::Leaf("audit".into()), Tile::Leaf("bash".into())],
            }
        );
        assert!(t.split("bash", "notes", SplitDir::Col));
        assert_eq!(t.leaves(), vec!["audit", "bash", "notes"]);
    }

    #[test]
    fn prune_flattens_and_drops() {
        let mut t = Tile::Split {
            dir: SplitDir::Row,
            weights: vec![1, 1],
            kids: vec![Tile::Leaf("a".into()), Tile::Leaf("b".into())],
        };
        let keep = HashSet::from(["a".into()]);
        assert!(t.prune(&keep));
        assert_eq!(t, Tile::Leaf("a".into()));
    }

    #[test]
    fn old_layout_json_has_empty_tiles() {
        let raw = r#"{"name":"default","catalog":"default"}"#;
        let layout: Layout = serde_json::from_str(raw).unwrap();
        assert!(layout.tiles.is_empty());
    }

    #[test]
    fn drag_pair_keeps_the_other_sibling() {
        assert_eq!(clamp_pair(10, 10, 4, 3), (14, 6));
        assert_eq!(clamp_pair(10, 10, 20, 3), (17, 3));
        assert_eq!(clamp_pair(10, 10, -20, 3), (3, 17));
        let mut w = vec![10, 10, 8];
        assert!(apply_gap(&mut w, 0, 10, 10, 4, 3));
        assert_eq!(w, vec![14, 6, 8]);
    }

    #[test]
    fn at_mut_reaches_a_nested_split() {
        let mut t = Tile::Split {
            dir: SplitDir::Col,
            weights: vec![1, 1],
            kids: vec![
                Tile::Leaf("a".into()),
                Tile::Split {
                    dir: SplitDir::Row,
                    weights: vec![1, 1],
                    kids: vec![Tile::Leaf("b".into()), Tile::Leaf("c".into())],
                },
            ],
        };
        t.at_mut(&[1]).unwrap().set_gap(0, 20, 20, -5);
        match t.at_mut(&[1]) {
            Some(Tile::Split { weights, .. }) => assert_eq!(weights, &vec![15, 25]),
            other => panic!("{other:?}"),
        }
        t.at_mut(&[]).unwrap().equalize();
        match &t {
            Tile::Split { weights, .. } => assert_eq!(weights, &vec![1, 1]),
            other => panic!("{other:?}"),
        }
    }
}
