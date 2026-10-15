//! `manifest.json`: the contract published by unity-explorer `scripts/delta` for every release
//! and platform, read here before any of its paths reaches the file system.
//!
//! The manifest is not signed, so every value is checked twice: once on the producer side when
//! the build is scanned, and again here. Value types (`BuildPath`, `LinkTarget`, `Sha256Digest`,
//! `FileMode`) refuse an invalid value when they are deserialized; [`Manifest::validate`] applies
//! the rules that span entries (link containment, nesting, base consistency).
//!
//! The rules are the producer's, applied on every platform: a manifest the producer accepted is
//! accepted here and nothing else is, whichever OS scanned the build and whichever installs it.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;
use std::str::FromStr;
use thiserror::Error;

use crate::utils::get_os_name;

/// Name of the manifest under `releases/<tag>/delta/<platform>/`.
pub const MANIFEST_FILE_NAME: &str = "manifest.json";

#[derive(Debug, Error)]
pub enum ManifestError {
    /// Not JSON, not the expected shape, or a value that breaks its rule (serde reports the
    /// value rules through `try_from`).
    #[error("manifest cannot be read: {0}")]
    Json(#[from] serde_json::Error),
    /// A rule spanning entries, or a value checked outside deserialization.
    #[error("manifest breaks a rule: {0}")]
    Rule(String),
}

impl ManifestError {
    const fn broken_rule(message: String) -> Self {
        Self::Rule(message)
    }
}

/// A control character, `\` (a Windows separator), `:` (drive letters, NTFS streams) or one of
/// `< > " | ? *`, which Win32 refuses in a name.
fn is_unsafe_char(c: char) -> bool {
    c.is_control() || matches!(c, '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
}

/// `CON`, `PRN`, `AUX`, `NUL`, and `COM` or `LPT` followed by `0` to `9`, `¹`, `²` or `³`, with
/// or without an extension, ignoring spaces before it: Win32 opens the device instead of a file
/// of that name. Mirrors the producer's rule.
fn is_windows_device(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || stem
            .strip_prefix("COM")
            .or_else(|| stem.strip_prefix("LPT"))
            .is_some_and(|port| {
                matches!(
                    port,
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
}

/// One name between the slashes of a [`BuildPath`]: a file or folder name that every supported
/// file system stores as given.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PathComponent(String);

impl PathComponent {
    /// Why `name` cannot be a component, if it cannot.
    fn problem(name: &str) -> Option<&'static str> {
        if name.is_empty() {
            Some("is empty (a leading, trailing or doubled slash)")
        } else if name == "." || name == ".." {
            Some("is `.` or `..`")
        } else if name.contains('/') {
            Some("contains a slash")
        } else if name.ends_with(['.', ' ']) {
            Some("ends in a dot or a space, which NTFS drops")
        } else if is_windows_device(name) {
            Some("is a Windows device name")
        } else if name.chars().any(is_unsafe_char) {
            Some("contains a character Win32 refuses")
        } else {
            None
        }
    }
}

impl FromStr for PathComponent {
    type Err = ManifestError;

    fn from_str(name: &str) -> Result<Self, ManifestError> {
        match Self::problem(name) {
            Some(problem) => Err(ManifestError::broken_rule(format!(
                "path component {name:?} {problem}"
            ))),
            None => Ok(Self(name.to_owned())),
        }
    }
}

impl fmt::Display for PathComponent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A `/`-separated path inside a build, relative to its root, that can never leave it: every
/// component is a [`PathComponent`]. Mirrors the producer's `RelPath`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BuildPath(String);

impl BuildPath {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The path as NTFS and APFS compare names: case folded.
    fn case_folded(&self) -> String {
        self.0.to_lowercase()
    }

    /// Every folder above the entry, nearest first, case folded.
    fn case_folded_ancestors(&self) -> Vec<String> {
        Path::new(&self.case_folded())
            .ancestors()
            .skip(1)
            .filter_map(|ancestor| ancestor.to_str())
            .filter(|ancestor| !ancestor.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    /// How many folders lie between the root and the entry.
    fn folder_depth(&self) -> usize {
        self.0.matches('/').count()
    }
}

impl FromStr for BuildPath {
    type Err = ManifestError;

    fn from_str(text: &str) -> Result<Self, ManifestError> {
        let bad_component = text
            .split('/')
            .find_map(|name| PathComponent::problem(name).map(|problem| (name, problem)));
        match bad_component {
            Some((name, problem)) => Err(ManifestError::broken_rule(format!(
                "path {text:?}: component {name:?} {problem}"
            ))),
            None => Ok(Self(text.to_owned())),
        }
    }
}

impl TryFrom<String> for BuildPath {
    type Error = ManifestError;

    fn try_from(text: String) -> Result<Self, ManifestError> {
        text.parse()
    }
}

impl From<BuildPath> for String {
    fn from(path: BuildPath) -> Self {
        path.0
    }
}

impl fmt::Display for BuildPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a symlink points: relative, free of the characters [`PathComponent`] refuses, and
/// resolved from the link's own folder by [`LinkTarget::stays_inside`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LinkTarget(String);

impl LinkTarget {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether the target, resolved from the folder of `link`, stays inside the build: every
    /// `..` must have a folder left to climb out of.
    fn stays_inside(&self, link: &BuildPath) -> bool {
        self.0
            .split('/')
            .try_fold(link.folder_depth(), |depth, name| match name {
                "" | "." => Some(depth),
                ".." => depth.checked_sub(1),
                _ => Some(depth.saturating_add(1)),
            })
            .is_some()
    }
}

impl FromStr for LinkTarget {
    type Err = ManifestError;

    fn from_str(text: &str) -> Result<Self, ManifestError> {
        if text.is_empty() || text.starts_with('/') {
            return Err(ManifestError::broken_rule(format!(
                "link target {text:?} is absolute or empty"
            )));
        }
        if text.chars().any(is_unsafe_char) {
            return Err(ManifestError::broken_rule(format!(
                "link target {text:?} contains a character Win32 refuses"
            )));
        }
        Ok(Self(text.to_owned()))
    }
}

impl TryFrom<String> for LinkTarget {
    type Error = ManifestError;

    fn try_from(text: String) -> Result<Self, ManifestError> {
        text.parse()
    }
}

impl From<LinkTarget> for String {
    fn from(target: LinkTarget) -> Self {
        target.0
    }
}

impl fmt::Display for LinkTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A SHA-256 digest, written as 64 lowercase hex digits in the manifest.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl From<[u8; 32]> for Sha256Digest {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl FromStr for Sha256Digest {
    type Err = ManifestError;

    fn from_str(text: &str) -> Result<Self, ManifestError> {
        let not_lowercase_hex = || {
            ManifestError::broken_rule(format!("{text:?} is not a 64-digit lowercase hex sha256"))
        };
        if text.chars().any(|c| c.is_ascii_uppercase()) {
            return Err(not_lowercase_hex());
        }
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(text, &mut bytes).map_err(|_| not_lowercase_hex())?;
        Ok(Self(bytes))
    }
}

impl TryFrom<String> for Sha256Digest {
    type Error = ManifestError;

    fn try_from(text: String) -> Result<Self, ManifestError> {
        text.parse()
    }
}

impl From<Sha256Digest> for String {
    fn from(digest: Sha256Digest) -> Self {
        digest.to_string()
    }
}

impl fmt::Display for Sha256Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

/// Unix permission bits, written as three or four octal digits in the manifest (`755`, `4755`).
/// Pass [`FileMode::bits`] to `std::os::unix::fs::PermissionsExt::from_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct FileMode(u32);

impl FileMode {
    pub const fn bits(self) -> u32 {
        self.0
    }
}

impl FromStr for FileMode {
    type Err = ManifestError;

    fn from_str(text: &str) -> Result<Self, ManifestError> {
        let not_a_mode =
            || ManifestError::broken_rule(format!("{text:?} is not an octal file mode"));
        if !(3..=4).contains(&text.len()) || !text.chars().all(|c| ('0'..='7').contains(&c)) {
            return Err(not_a_mode());
        }
        u32::from_str_radix(text, 8)
            .map(Self)
            .map_err(|_| not_a_mode())
    }
}

impl TryFrom<String> for FileMode {
    type Error = ManifestError;

    fn try_from(text: String) -> Result<Self, ManifestError> {
        text.parse()
    }
}

impl From<FileMode> for String {
    fn from(mode: FileMode) -> Self {
        mode.to_string()
    }
}

impl fmt::Display for FileMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:03o}", self.0)
    }
}

/// The JSON the producer publishes, field for field. A change here is a change to the contract
/// pinned by `the_producer_contract_is_pinned`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: String,
    pub platform: String,
    pub zip: ZipRef,
    /// The complete new build, sorted by path. Folders exist only through the entries under them.
    pub files: Vec<FileEntry>,
    /// Patch sets keyed by the installed version they start from.
    pub bases: BTreeMap<String, BaseDelta>,
}

/// The published full zip the manifest was generated from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZipRef {
    pub path: String,
    pub size: u64,
    pub sha256: Sha256Digest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum FileEntry {
    File {
        path: BuildPath,
        size: u64,
        sha256: Sha256Digest,
        mode: FileMode,
    },
    Link {
        path: BuildPath,
        symlink: LinkTarget,
    },
}

impl FileEntry {
    pub const fn path(&self) -> &BuildPath {
        match self {
            Self::File { path, .. } | Self::Link { path, .. } => path,
        }
    }
}

/// What moves one base to the release: paths to delete, patches against base files, and whole
/// files encoded against an empty file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseDelta {
    pub deleted: Vec<BuildPath>,
    pub patches: Vec<PatchEntry>,
    pub whole: Vec<WholeEntry>,
}

impl BaseDelta {
    /// Every path rebuilt from a patch or a whole file.
    pub fn rebuilt(&self) -> impl Iterator<Item = &BuildPath> {
        self.patches
            .iter()
            .map(|p| &p.path)
            .chain(self.whole.iter().map(|w| &w.path))
    }

    /// Bytes to download for this base.
    pub fn download_bytes(&self) -> u64 {
        self.patches
            .iter()
            .map(|p| p.patch_size)
            .chain(self.whole.iter().map(|w| w.patch_size))
            .fold(0, u64::saturating_add)
    }
}

/// `path` of the new build is `from_path` of the installed build plus `patch`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PatchEntry {
    pub path: BuildPath,
    pub from_path: BuildPath,
    pub from_sha256: Sha256Digest,
    /// Relative to the manifest's folder.
    pub patch: BuildPath,
    pub patch_size: u64,
    pub patch_sha256: Sha256Digest,
}

/// `path` of the new build is `patch` applied to an empty file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WholeEntry {
    pub path: BuildPath,
    /// Relative to the manifest's folder.
    pub patch: BuildPath,
    pub patch_size: u64,
    pub patch_sha256: Sha256Digest,
}

impl Manifest {
    /// Reads a manifest meant for `version` on this platform and applies every rule.
    pub fn parse(json: &str, version: &str) -> Result<Self, ManifestError> {
        let manifest: Self = serde_json::from_str(json)?;
        manifest.validate(version)?;
        Ok(manifest)
    }

    /// The rules that span entries, for the platform this launcher runs on. Value rules are
    /// applied when the manifest is deserialized.
    pub fn validate(&self, version: &str) -> Result<(), ManifestError> {
        self.validate_for(get_os_name(), version)
    }

    fn validate_for(&self, platform: &str, version: &str) -> Result<(), ManifestError> {
        if self.platform != platform {
            return Err(ManifestError::broken_rule(format!(
                "manifest is for platform {:?}, not {platform:?}",
                self.platform
            )));
        }
        if self.version != version {
            return Err(ManifestError::broken_rule(format!(
                "manifest is for version {:?}, not {version:?}",
                self.version
            )));
        }
        if let Some(problem) = PathComponent::problem(&self.zip.path) {
            return Err(ManifestError::broken_rule(format!(
                "zip path {:?} {problem}",
                self.zip.path
            )));
        }

        let files = self.checked_files()?;
        for (base, delta) in &self.bases {
            Self::check_base(base, delta, &files, version)?;
        }
        Ok(())
    }

    /// `files` by path, refusing duplicates (ignoring case), an entry under a file or link
    /// (ignoring case) and a link that leaves the build.
    fn checked_files(&self) -> Result<BTreeMap<&BuildPath, &FileEntry>, ManifestError> {
        let mut by_path: BTreeMap<&BuildPath, &FileEntry> = BTreeMap::new();
        let mut case_folded: BTreeSet<String> = BTreeSet::new();
        for entry in &self.files {
            let path = entry.path();
            let is_new =
                case_folded.insert(path.case_folded()) && by_path.insert(path, entry).is_none();
            if !is_new {
                return Err(ManifestError::broken_rule(format!(
                    "files lists {path} more than once (ignoring case)"
                )));
            }
        }
        for entry in &self.files {
            let path = entry.path();
            let leaf_ancestor = path
                .case_folded_ancestors()
                .into_iter()
                .find(|ancestor| case_folded.contains(ancestor));
            if let Some(parent) = leaf_ancestor {
                return Err(ManifestError::broken_rule(format!(
                    "{path} lies under {parent}, which is a file or link"
                )));
            }
            if let FileEntry::Link { symlink, .. } = entry
                && !symlink.stays_inside(path)
            {
                return Err(ManifestError::broken_rule(format!(
                    "link {path} -> {symlink} leaves the build"
                )));
            }
        }
        Ok(by_path)
    }

    fn check_base(
        base: &str,
        delta: &BaseDelta,
        files: &BTreeMap<&BuildPath, &FileEntry>,
        version: &str,
    ) -> Result<(), ManifestError> {
        if base == version {
            return Err(ManifestError::broken_rule(format!(
                "{version} is listed as its own base"
            )));
        }
        for path in &delta.deleted {
            if files.contains_key(path) {
                return Err(ManifestError::broken_rule(format!(
                    "base {base}: {path} is deleted but listed in files"
                )));
            }
        }
        let mut rebuilt: BTreeSet<&BuildPath> = BTreeSet::new();
        for path in delta.rebuilt() {
            if !matches!(files.get(path), Some(FileEntry::File { .. })) {
                return Err(ManifestError::broken_rule(format!(
                    "base {base}: {path} is rebuilt but is not a file of the release build"
                )));
            }
            if !rebuilt.insert(path) {
                return Err(ManifestError::broken_rule(format!(
                    "base {base}: {path} is rebuilt more than once"
                )));
            }
        }
        Ok(())
    }

    /// The patch set starting from `installed`, if the manifest lists it.
    pub fn base(&self, installed: &str) -> Option<&BaseDelta> {
        self.bases.get(installed)
    }

    /// The entry of the new build at `path`.
    pub fn file(&self, path: &BuildPath) -> Option<&FileEntry> {
        self.files.iter().find(|entry| entry.path() == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Result, ensure};
    use rstest::rstest;

    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// The literal from the producer's `publishing/contract.rs`: the exact JSON the launcher reads.
    fn producer_fixture() -> String {
        format!(
            r#"{{"version":"v1.1.0-alpha","platform":"windows64","zip":{{"path":"Decentraland_windows64.zip","size":3,"sha256":"{SHA}"}},"files":[{{"path":"a.dll","size":1,"sha256":"{SHA}","mode":"644"}},{{"path":"l","symlink":"a.dll"}}],"bases":{{"v1.0.0-alpha":{{"deleted":["b"],"patches":[{{"path":"a.dll","from_path":"a.dll","from_sha256":"{SHA}","patch":"from/v1.0.0-alpha/aaaaaaaa/a.dll.hdiff","patch_size":1,"patch_sha256":"{SHA}"}}],"whole":[{{"path":"c","patch":"from/v1.0.0-alpha/aaaaaaaa/c.hdiff","patch_size":2,"patch_sha256":"{SHA}"}}]}}}}}}"#
        )
    }

    #[test]
    fn the_producer_contract_is_pinned() -> Result<()> {
        let json = producer_fixture();
        let manifest: Manifest = serde_json::from_str(&json)?;
        ensure!(
            serde_json::to_value(&manifest)? == serde_json::from_str::<serde_json::Value>(&json)?,
            "the DTO must round-trip the producer's JSON byte for byte"
        );
        ensure!(manifest.files.len() == 2);
        ensure!(manifest.base("v1.0.0-alpha").is_some());
        ensure!(manifest.base("v0.9.0-alpha").is_none());
        Ok(())
    }

    #[test]
    fn the_fixture_fails_only_its_own_cross_entry_rule() -> Result<()> {
        // `c` is rebuilt from a whole file but is not in `files`: the producer's literal pins the
        // JSON shape, not a consistent release, so validation must say exactly that.
        let manifest: Manifest = serde_json::from_str(&producer_fixture())?;
        let error = manifest
            .validate_for("windows64", "v1.1.0-alpha")
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        ensure!(error.contains("c is rebuilt but is not a file"), "{error}");
        Ok(())
    }

    #[test]
    fn unknown_fields_are_refused() {
        let json = producer_fixture().replacen("\"version\"", "\"extra\":1,\"version\"", 1);
        assert!(serde_json::from_str::<Manifest>(&json).is_err());
    }

    #[rstest]
    #[case("a")]
    #[case("Data/space dir/asset file.bin")]
    #[case("x.app/Contents/Info.plist")]
    #[case(".hidden")]
    #[case("COM10")]
    #[case("CONFIG")]
    #[case("console.log")]
    #[case("lpt")]
    #[case("a.b.c")]
    fn build_path_accepts(#[case] path: &str) -> Result<()> {
        ensure!(path.parse::<BuildPath>()?.as_str() == path);
        Ok(())
    }

    #[rstest]
    #[case("")]
    #[case("/etc/passwd")]
    #[case("a/../b")]
    #[case("..")]
    #[case("./a")]
    #[case("a//b")]
    #[case("a/")]
    #[case("..\\..\\x.dll")]
    #[case("C:x.dll")]
    #[case("\\\\host\\share\\x")]
    #[case("Decentraland.exe:ads")]
    #[case("a\u{0}b")]
    #[case("a\nb")]
    #[case("a\tb")]
    #[case("Decentraland.exe.")]
    #[case("Foo.dll ")]
    #[case("sub/trailing. /x")]
    #[case("CON")]
    #[case("con.txt")]
    #[case("Data/aux")]
    #[case("COM1")]
    #[case("lpt9.log")]
    #[case("COM0")]
    #[case("lpt³.txt")]
    #[case("Data/COM¹.dll")]
    #[case("CON .txt")]
    #[case("Data/nul  .dll")]
    #[case("COM1 .log")]
    #[case("a<b")]
    #[case("a>b")]
    #[case("a\"b")]
    #[case("a|b")]
    #[case("a?b")]
    #[case("a*b")]
    fn build_path_refuses(#[case] path: &str) {
        assert!(path.parse::<BuildPath>().is_err(), "accepted {path:?}");
    }

    #[test]
    fn build_path_is_checked_when_read_from_json() {
        let read: Result<Vec<BuildPath>, serde_json::Error> =
            serde_json::from_str(r#"["ok", "C:x"]"#);
        assert!(read.is_err());
    }

    #[test]
    fn sha256_digest_rules_and_round_trip() -> Result<()> {
        let digest: Sha256Digest = "ab".repeat(32).parse()?;
        ensure!(digest.as_bytes().iter().all(|b| *b == 0xab));
        ensure!(digest.to_string() == "ab".repeat(32));
        ensure!("A".repeat(64).parse::<Sha256Digest>().is_err());
        ensure!("a".repeat(63).parse::<Sha256Digest>().is_err());
        ensure!("g".repeat(64).parse::<Sha256Digest>().is_err());
        Ok(())
    }

    #[test]
    fn file_mode_rules_and_round_trip() -> Result<()> {
        ensure!("755".parse::<FileMode>()?.bits() == 0o755);
        ensure!("755".parse::<FileMode>()?.to_string() == "755");
        ensure!("4755".parse::<FileMode>()?.to_string() == "4755");
        ensure!("999".parse::<FileMode>().is_err());
        ensure!("75".parse::<FileMode>().is_err());
        Ok(())
    }

    #[rstest]
    #[case("sub/link", "../sibling", true)]
    #[case("sub/up", "..", true)]
    #[case("Fw/Versions/Current", "A", true)]
    #[case("sub/link", "../../outside", false)]
    #[case("link", "..", false)]
    #[case("a/b", "./../../x", false)]
    #[case("a/b", "c/../../..", false)]
    #[case("sub/link", "/etc/passwd", false)]
    #[case("sub/link", "", false)]
    #[case("sub/link", "x\nF\tevil", false)]
    #[case("sub/link", "..\\x", false)]
    #[case("sub/link", "C:evil", false)]
    fn link_containment(
        #[case] link: &str,
        #[case] target: &str,
        #[case] allowed: bool,
    ) -> Result<()> {
        let link: BuildPath = link.parse()?;
        let contained = target
            .parse::<LinkTarget>()
            .is_ok_and(|target| target.stays_inside(&link));
        ensure!(contained == allowed, "{link} -> {target}");
        Ok(())
    }

    fn file(path: &str) -> serde_json::Value {
        serde_json::json!({ "path": path, "size": 1, "sha256": SHA, "mode": "644" })
    }

    fn link(path: &str, target: &str) -> serde_json::Value {
        serde_json::json!({ "path": path, "symlink": target })
    }

    fn whole(path: &str) -> serde_json::Value {
        serde_json::json!({ "path": path, "patch": format!("from/v1.0.0/deadbeef/{path}.hdiff"), "patch_size": 1, "patch_sha256": SHA })
    }

    fn patch(path: &str, from_path: &str) -> serde_json::Value {
        serde_json::json!({ "path": path, "from_path": from_path, "from_sha256": SHA, "patch": format!("from/v1.0.0/deadbeef/{path}.hdiff"), "patch_size": 1, "patch_sha256": SHA })
    }

    /// A release manifest for this platform with one base, `v1.0.0`.
    fn manifest(files: &[serde_json::Value], base: &serde_json::Value) -> String {
        serde_json::json!({
            "version": "v1.1.0",
            "platform": get_os_name(),
            "zip": { "path": "Decentraland_windows64.zip", "size": 1, "sha256": SHA },
            "files": files,
            "bases": { "v1.0.0": base },
        })
        .to_string()
    }

    fn base(
        deleted: &[&str],
        patches: &[serde_json::Value],
        whole: &[serde_json::Value],
    ) -> serde_json::Value {
        serde_json::json!({ "deleted": deleted, "patches": patches, "whole": whole })
    }

    fn parse(json: &str) -> Result<Manifest, String> {
        Manifest::parse(json, "v1.1.0").map_err(|e| e.to_string())
    }

    #[test]
    fn accepts_a_consistent_release() -> Result<()> {
        let json = manifest(
            &[
                file("a.dll"),
                file("Data/new.bin"),
                file("Fw/Versions/A/lib"),
                link("Fw/Versions/Current", "A"),
            ],
            &base(
                &["gone.dll"],
                &[patch("a.dll", "a.dll")],
                &[whole("Data/new.bin")],
            ),
        );
        let manifest = parse(&json).map_err(anyhow::Error::msg)?;
        let delta = manifest
            .base("v1.0.0")
            .ok_or_else(|| anyhow::anyhow!("base"))?;
        ensure!(delta.download_bytes() == 2);
        ensure!(delta.rebuilt().count() == 2);
        ensure!(matches!(
            manifest.file(&"Fw/Versions/Current".parse()?),
            Some(FileEntry::Link { .. })
        ));
        Ok(())
    }

    #[test]
    fn refuses_another_platform_or_version() -> Result<()> {
        let json = manifest(&[file("a")], &base(&[], &[], &[]));
        let manifest: Manifest = serde_json::from_str(&json)?;
        let other_platform = manifest.validate_for("other", "v1.1.0");
        ensure!(other_platform.is_err_and(|e| e.to_string().contains("platform")));
        let other_version = manifest.validate("v1.2.0");
        ensure!(other_version.is_err_and(|e| e.to_string().contains("version")));
        Ok(())
    }

    #[test]
    fn refuses_the_release_as_its_own_base() {
        let json =
            manifest(&[file("a")], &base(&[], &[], &[])).replacen("\"v1.0.0\"", "\"v1.1.0\"", 1);
        assert!(parse(&json).is_err_and(|e| e.contains("its own base")));
    }

    #[test]
    fn refuses_a_deleted_path_still_in_files() {
        let json = manifest(&[file("a")], &base(&["a"], &[], &[]));
        assert!(parse(&json).is_err_and(|e| e.contains("deleted but listed")));
    }

    #[test]
    fn refuses_a_rebuilt_path_that_is_not_a_file() {
        let missing = manifest(&[file("a")], &base(&[], &[], &[whole("b")]));
        assert!(parse(&missing).is_err_and(|e| e.contains("not a file")));
        let is_link = manifest(
            &[file("a"), link("l", "a")],
            &base(&[], &[patch("l", "l")], &[]),
        );
        assert!(parse(&is_link).is_err_and(|e| e.contains("not a file")));
    }

    #[test]
    fn refuses_a_path_rebuilt_twice() {
        let json = manifest(&[file("a")], &base(&[], &[patch("a", "a")], &[whole("a")]));
        assert!(parse(&json).is_err_and(|e| e.contains("more than once")));
    }

    #[test]
    fn refuses_duplicate_files_including_case_variants() {
        let json = manifest(
            &[file("Plugin.dll"), file("plugin.dll")],
            &base(&[], &[], &[]),
        );
        assert!(parse(&json).is_err_and(|e| e.contains("more than once")));
    }

    #[test]
    fn refuses_an_entry_under_a_file_or_link_whatever_the_case() {
        let under_file = manifest(&[file("a"), file("A/b")], &base(&[], &[], &[]));
        assert!(parse(&under_file).is_err_and(|e| e.contains("lies under")));
        let under_link = manifest(&[link("Lib", "x"), file("lib/evil")], &base(&[], &[], &[]));
        assert!(parse(&under_link).is_err_and(|e| e.contains("lies under")));
    }

    #[test]
    fn refuses_a_link_leaving_the_build() {
        let json = manifest(&[link("l", "../escape")], &base(&[], &[], &[]));
        assert!(parse(&json).is_err_and(|e| e.contains("leaves the build")));
    }

    #[test]
    fn refuses_an_unsafe_path_anywhere() {
        let in_deleted = manifest(&[file("a")], &base(&["..\\x"], &[], &[]));
        assert!(parse(&in_deleted).is_err());
        let in_patch_key = manifest(&[file("a")], &base(&[], &[], &[whole("a")])).replacen(
            "from/v1.0.0/deadbeef/a.hdiff",
            "../a.hdiff",
            1,
        );
        assert!(parse(&in_patch_key).is_err());
        let in_zip = manifest(&[file("a")], &base(&[], &[], &[])).replacen(
            "Decentraland_windows64.zip",
            "x/Decentraland_windows64.zip",
            1,
        );
        assert!(parse(&in_zip).is_err_and(|e| e.contains("zip path")));
    }
}
