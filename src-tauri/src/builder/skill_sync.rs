//! Native Claude Code skills for Alfred's shared `.agents/skills` library.
//!
//! Only user-level, agent-neutral skills are linked. Provider-owned folders
//! (`.claude/skills`, `.cursor/skills`, …) are never copied or removed.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const MAX_FILES: usize = 2_000;
const MAX_BYTES: u64 = 32 * 1024 * 1024;
const MARKER: &str = ".alfred-sync";

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillSyncResult {
    pub linked: usize,
    pub copied: usize,
    pub removed: usize,
    pub skipped: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlfredSkill {
    pub name: String,
    pub path: String,
    pub linked: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct SyncManifest {
    #[serde(default)]
    linked: Vec<String>,
    #[serde(default)]
    copied: Vec<String>,
}

/// Link Alfred's shared user skills into Claude Code's user skill folder.
#[tauri::command]
pub fn builder_sync_claude_skills() -> Result<SkillSyncResult, String> {
    let home = directories::BaseDirs::new()
        .ok_or("No home folder is available.")?
        .home_dir()
        .to_path_buf();
    sync_shared_skills(&home)
}

/// The user-level `.agents/skills` library, with its Claude Code link state.
#[tauri::command]
pub fn builder_alfred_skills() -> Result<Vec<AlfredSkill>, String> {
    let home = directories::BaseDirs::new()
        .ok_or("No home folder is available.")?
        .home_dir()
        .to_path_buf();
    alfred_skills(&home)
}

fn alfred_skills(home: &Path) -> Result<Vec<AlfredSkill>, String> {
    let source_root = home.join(".agents/skills");
    let target_root = home.join(".claude/skills");
    let (entries, _) = shared_skill_dirs(&source_root)?;
    Ok(entries
        .into_iter()
        .map(|(name, path)| AlfredSkill {
            linked: is_managed_skill(&target_root.join(&name), &source_root),
            path: path.join("SKILL.md").to_string_lossy().into_owned(),
            name,
        })
        .collect())
}

fn is_managed_skill(target: &Path, source_root: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(target) else {
        return false;
    };
    if metadata.file_type().is_symlink() {
        return link_points_into(target, source_root);
    }
    metadata.is_dir()
        && copy_marker_source(target).is_some_and(|source| source.starts_with(source_root))
}

fn sync_shared_skills(home: &Path) -> Result<SkillSyncResult, String> {
    let source_root = home.join(".agents/skills");
    let target_root = home.join(".claude/skills");
    let manifest_path = home.join(".agents/.alfred-claude-skills.json");
    let (entries, skipped) = shared_skill_dirs(&source_root)?;
    fs::create_dir_all(&target_root).map_err(|error| error.to_string())?;
    let mut manifest = load_manifest(&manifest_path);
    let current: HashSet<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
    let mut result = SkillSyncResult {
        skipped,
        ..Default::default()
    };

    for name in manifest.linked.iter().chain(manifest.copied.iter()) {
        if current.contains(name.as_str()) {
            continue;
        }
        let target = target_root.join(name);
        if remove_managed_link(&target, &source_root) || remove_managed_copy(&target, &source_root)
        {
            result.removed += 1;
        }
    }

    let mut linked = Vec::new();
    let mut copied = Vec::new();
    for (name, source) in entries {
        let target = target_root.join(&name);
        if let Ok(metadata) = fs::symlink_metadata(&target) {
            if metadata.file_type().is_symlink() {
                if link_points_into(&target, &source_root) {
                    linked.push(name);
                } else {
                    result
                        .skipped
                        .push(format!("{name}: existing symlink is not managed by Alfred"));
                }
                continue;
            }
            if metadata.is_dir()
                && copy_marker_source(&target)
                    .is_some_and(|source_path| source_path.starts_with(&source_root))
            {
                if fs::remove_dir_all(&target).is_err() {
                    result
                        .skipped
                        .push(format!("{name}: previous copy could not be replaced"));
                    continue;
                }
            } else {
                result.skipped.push(format!(
                    "{name}: already exists and is not managed by Alfred"
                ));
                continue;
            }
        }

        match create_link(&source, &target) {
            Ok(()) => linked.push(name),
            Err(link_error) => match copy_skill(&source, &target) {
                Ok(()) => copied.push(name),
                Err(copy_error) => result
                    .skipped
                    .push(format!("{name}: {link_error}; {copy_error}")),
            },
        }
    }

    manifest = SyncManifest {
        linked: linked.clone(),
        copied: copied.clone(),
    };
    save_manifest(&manifest_path, &manifest)?;
    result.linked = linked.len();
    result.copied = copied.len();
    Ok(result)
}

fn shared_skill_dirs(source_root: &Path) -> Result<(Vec<(String, PathBuf)>, Vec<String>), String> {
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    let read = match fs::read_dir(source_root) {
        Ok(read) => read,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((entries, skipped))
        }
        Err(error) => return Err(error.to_string()),
    };
    for entry in read {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if !path.is_dir() || !path.join("SKILL.md").is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !valid_target_name(&name) {
            skipped.push(format!(
                "{name}: folder name cannot be used as a Claude Code skill name"
            ));
            continue;
        }
        entries.push((name, path));
    }
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    Ok((entries, skipped))
}

fn valid_target_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
}

fn load_manifest(path: &Path) -> SyncManifest {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_manifest(path: &Path, manifest: &SyncManifest) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn link_points_into(target: &Path, source_root: &Path) -> bool {
    let Ok(link) = fs::read_link(target) else {
        return false;
    };
    let link = if link.is_absolute() {
        link
    } else {
        target
            .parent()
            .map(|parent| parent.join(&link))
            .unwrap_or(link)
    };
    let root = source_root
        .canonicalize()
        .unwrap_or_else(|_| source_root.to_path_buf());
    link.canonicalize()
        .map(|path| path.starts_with(&root))
        .unwrap_or_else(|_| normalize_lexically(&link).starts_with(source_root))
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn remove_managed_link(target: &Path, source_root: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(target) else {
        return false;
    };
    if !metadata.file_type().is_symlink() || !link_points_into(target, source_root) {
        return false;
    }
    fs::remove_file(target).is_ok()
}

fn copy_marker_source(target: &Path) -> Option<PathBuf> {
    fs::read_to_string(target.join(MARKER))
        .ok()
        .map(PathBuf::from)
}

fn remove_managed_copy(target: &Path, source_root: &Path) -> bool {
    let Some(source) = copy_marker_source(target) else {
        return false;
    };
    if !source.starts_with(source_root) {
        return false;
    }
    fs::remove_dir_all(target).is_ok()
}

#[cfg(unix)]
fn create_link(source: &Path, target: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(source, target).map_err(|error| error.to_string())
}

#[cfg(windows)]
fn create_link(source: &Path, target: &Path) -> Result<(), String> {
    std::os::windows::fs::symlink_dir(source, target).map_err(|error| error.to_string())
}

#[cfg(not(any(unix, windows)))]
fn create_link(_source: &Path, _target: &Path) -> Result<(), String> {
    Err("Skill links are not supported on this platform.".into())
}

#[derive(Default)]
struct CopyBudget {
    files: usize,
    bytes: u64,
}

impl CopyBudget {
    fn take(&mut self, size: u64) -> Result<(), String> {
        self.files += 1;
        self.bytes = self.bytes.saturating_add(size);
        if self.files > MAX_FILES {
            return Err("Skill has too many files.".into());
        }
        if self.bytes > MAX_BYTES {
            return Err("Skill is larger than 32 MB.".into());
        }
        Ok(())
    }
}

fn copy_skill(source: &Path, target: &Path) -> Result<(), String> {
    let mut budget = CopyBudget::default();
    if let Err(error) = copy_tree(source, target, &mut budget) {
        let _ = fs::remove_dir_all(target);
        return Err(error);
    }
    if let Err(error) = fs::write(target.join(MARKER), source.to_string_lossy().as_bytes()) {
        let _ = fs::remove_dir_all(target);
        return Err(error.to_string());
    }
    if !target.join("SKILL.md").is_file() {
        let _ = fs::remove_dir_all(target);
        return Err("The copied skill is incomplete.".into());
    }
    Ok(())
}

fn copy_tree(source: &Path, target: &Path, budget: &mut CopyBudget) -> Result<(), String> {
    let metadata = fs::symlink_metadata(source).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() {
        return Err("Skill packages cannot contain symlinks.".into());
    }
    if metadata.is_file() {
        budget.take(metadata.len())?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::copy(source, target).map_err(|error| error.to_string())?;
        return Ok(());
    }
    if !metadata.is_dir() {
        return Err("Skill packages may only contain files and folders.".into());
    }
    fs::create_dir_all(target).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        if name == std::ffi::OsStr::new(".git") || name == std::ffi::OsStr::new(MARKER) {
            continue;
        }
        copy_tree(&entry.path(), &target.join(name), budget)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHome(PathBuf);

    impl TestHome {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("alfred-skill-sync-{name}-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("home");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }

        fn add_skill(&self, name: &str) -> PathBuf {
            let directory = self.0.join(".agents/skills").join(name);
            fs::create_dir_all(&directory).expect("skill dir");
            fs::write(
                directory.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: Test\n---\nBody\n"),
            )
            .expect("skill");
            directory
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn links_only_shared_skills_and_removes_stale_links() {
        let home = TestHome::new("links");
        let first = home.add_skill("first");
        home.add_skill("second");
        let result = sync_shared_skills(home.path()).expect("sync");
        assert_eq!(result.linked + result.copied, 2);
        assert!(home.path().join(".claude/skills/first/SKILL.md").is_file());
        assert!(home.path().join(".claude/skills/second/SKILL.md").is_file());
        let listed = alfred_skills(home.path()).expect("list");
        assert_eq!(listed.len(), 2);
        assert!(listed.iter().all(|skill| skill.linked));

        let again = sync_shared_skills(home.path()).expect("second sync");
        assert_eq!(again.linked + again.copied, 2);
        assert!(again.skipped.is_empty());

        fs::remove_dir_all(first).expect("remove source");
        let third = sync_shared_skills(home.path()).expect("third sync");
        assert_eq!(third.removed, 1);
        assert!(!home.path().join(".claude/skills/first").exists());
        assert!(home.path().join(".claude/skills/second/SKILL.md").is_file());
        assert_eq!(alfred_skills(home.path()).expect("list").len(), 1);
    }

    #[test]
    fn never_overwrites_or_removes_provider_owned_skills() {
        let home = TestHome::new("provider");
        home.add_skill("shared");
        let provider = home.path().join(".claude/skills/provider");
        fs::create_dir_all(&provider).expect("provider skill");
        fs::write(provider.join("SKILL.md"), "---\nname: provider\n---\n").expect("provider file");

        let conflict = home.path().join(".claude/skills/shared");
        fs::create_dir_all(&conflict).expect("conflict");
        fs::write(conflict.join("SKILL.md"), "user-owned").expect("conflict file");

        let result = sync_shared_skills(home.path()).expect("sync");
        assert!(result.skipped.iter().any(|item| item.contains("shared")));
        assert!(alfred_skills(home.path())
            .expect("list")
            .iter()
            .all(|skill| !skill.linked));
        assert_eq!(
            fs::read_to_string(conflict.join("SKILL.md")).expect("read"),
            "user-owned"
        );
        assert!(provider.join("SKILL.md").is_file());

        fs::remove_dir_all(home.path().join(".agents/skills/shared")).expect("remove source");
        let next = sync_shared_skills(home.path()).expect("next sync");
        assert_eq!(next.removed, 0);
        assert!(provider.join("SKILL.md").is_file());
        assert!(conflict.join("SKILL.md").is_file());
    }
}
