//! One installable unit, from a Git registry or a static index.

use crate::registry::UnitFile;

pub(super) struct Spec {
    pub id: String,
    pub version: String,
    pub kind: String,
    pub sha256: String,
    pub source: String,
    pub commit: String,
    pub files: Vec<UnitFile>,
    pub license: String,
    pub requires: Vec<String>,
}
