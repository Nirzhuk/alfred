//! Text-file access for Alfred's built-in file editor.

use serde::Serialize;
use std::fs::{self, Metadata, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// A textarea stops being pleasant to edit in around this size.
const MAX_EDITABLE_BYTES: u64 = 2 * 1024 * 1024;

/// The frontend matches this exact string to offer Reload or Overwrite.
const CHANGED_ON_DISK: &str = "FILE_CHANGED_ON_DISK";

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TextFile {
    content: String,
    modified_ms: u64,
}

fn absolute(path: &str) -> Result<&Path, String> {
    let path = Path::new(path);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err("Alfred can only edit files with an absolute path.".into())
    }
}

fn modified_ms(meta: &Metadata) -> Result<u64, String> {
    let modified = meta.modified().map_err(|e| e.to_string())?;
    let elapsed = modified.duration_since(UNIX_EPOCH).unwrap_or_default();
    Ok(elapsed.as_millis() as u64)
}

fn read_text(path: &str) -> Result<TextFile, String> {
    let path = absolute(path)?;
    let meta = fs::metadata(path).map_err(|e| format!("Could not open this file: {e}"))?;
    if meta.is_dir() {
        return Err("This is a folder, so there is nothing to edit here.".into());
    }
    if !meta.is_file() {
        return Err("This is not a regular file.".into());
    }
    if meta.len() > MAX_EDITABLE_BYTES {
        return Err("This file is larger than 2 MB, too large to edit in Alfred.".into());
    }
    let bytes = fs::read(path).map_err(|e| format!("Could not read this file: {e}"))?;
    if bytes.contains(&0) {
        return Err("This is not a text file, so Alfred cannot edit it.".into());
    }
    let content = String::from_utf8(bytes)
        .map_err(|_| "This file is not UTF-8 text, so Alfred cannot edit it.".to_string())?;
    Ok(TextFile {
        content,
        modified_ms: modified_ms(&meta)?,
    })
}

fn temp_sibling(target: &Path) -> Result<PathBuf, String> {
    let name = target
        .file_name()
        .ok_or_else(|| "This path has no file name.".to_string())?
        .to_string_lossy();
    let nanos = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    Ok(target.with_file_name(format!(".{name}.alfred-{nanos}.tmp")))
}

/// Replaces the file atomically, so a failed save leaves the original intact.
/// `expected_modified_ms` is what the editor loaded; `None` overwrites anyway.
fn write_text(
    path: &str,
    content: &str,
    expected_modified_ms: Option<u64>,
) -> Result<u64, String> {
    // Resolve symlinks so the save replaces the real file, not the link.
    let target = fs::canonicalize(absolute(path)?)
        .map_err(|e| format!("Could not find this file: {e}"))?;
    let meta = fs::metadata(&target).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("This is not a regular file.".into());
    }
    if meta.permissions().readonly() {
        return Err("This file is read-only.".into());
    }
    if expected_modified_ms.is_some_and(|expected| expected != modified_ms(&meta).unwrap_or(0)) {
        return Err(CHANGED_ON_DISK.into());
    }

    let temp = temp_sibling(&target)?;
    let replace = || -> std::io::Result<()> {
        let mut file = OpenOptions::new().write(true).create_new(true).open(&temp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        fs::set_permissions(&temp, meta.permissions())?;
        fs::rename(&temp, &target)
    };
    if let Err(e) = replace() {
        let _ = fs::remove_file(&temp);
        return Err(format!("Could not save this file: {e}"));
    }
    let saved = fs::metadata(&target).map_err(|e| e.to_string())?;
    modified_ms(&saved)
}

#[tauri::command(async)]
pub fn read_text_file(path: String) -> Result<TextFile, String> {
    read_text(&path)
}

#[tauri::command(async)]
pub fn write_text_file(
    path: String,
    content: String,
    expected_modified_ms: Option<u64>,
) -> Result<u64, String> {
    write_text(&path, &content, expected_modified_ms)
}

/// Dropped paths arrive with no type; a missing path counts as not a folder.
#[tauri::command(async)]
pub fn paths_are_folders(paths: Vec<String>) -> Vec<bool> {
    paths.iter().map(|path| Path::new(path).is_dir()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(test: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "alfred-files-{test}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn text(path: &Path) -> String {
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn saves_and_leaves_no_temp_file_behind() {
        let dir = scratch("save");
        let file = dir.join("blocks.json");
        fs::write(&file, "old").unwrap();
        let loaded = read_text(&text(&file)).unwrap();
        assert_eq!(loaded.content, "old");

        write_text(&text(&file), "new", Some(loaded.modified_ms)).unwrap();

        assert_eq!(fs::read_to_string(&file).unwrap(), "new");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn refuses_to_overwrite_a_file_changed_since_it_was_read() {
        let dir = scratch("conflict");
        let file = dir.join("notes.md");
        fs::write(&file, "theirs").unwrap();
        let stale = modified_ms(&fs::metadata(&file).unwrap()).unwrap() - 1000;

        let refused = write_text(&text(&file), "mine", Some(stale));

        assert_eq!(refused.unwrap_err(), CHANGED_ON_DISK);
        assert_eq!(fs::read_to_string(&file).unwrap(), "theirs");

        write_text(&text(&file), "mine", None).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "mine");
    }

    #[test]
    fn refuses_folders_binary_and_relative_paths() {
        let dir = scratch("refuse");
        let binary = dir.join("image.bin");
        fs::write(&binary, [1u8, 0, 2]).unwrap();
        let latin1 = dir.join("latin1.txt");
        fs::write(&latin1, [0xe9u8]).unwrap();

        assert!(read_text(&text(&dir)).unwrap_err().contains("folder"));
        assert!(read_text(&text(&binary)).unwrap_err().contains("not a text"));
        assert!(read_text(&text(&latin1)).unwrap_err().contains("UTF-8"));
        assert!(read_text("relative.txt").unwrap_err().contains("absolute"));
        assert!(write_text("relative.txt", "x", None).unwrap_err().contains("absolute"));
    }

    #[test]
    fn refuses_to_replace_a_read_only_file() {
        let dir = scratch("readonly");
        let file = dir.join("locked.txt");
        fs::write(&file, "keep").unwrap();
        let mut permissions = fs::metadata(&file).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&file, permissions).unwrap();

        assert!(write_text(&text(&file), "x", None).unwrap_err().contains("read-only"));
        assert_eq!(fs::read_to_string(&file).unwrap(), "keep");
    }

    #[cfg(unix)]
    #[test]
    fn keeps_symlinks_and_permissions() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let dir = scratch("symlink");
        let real = dir.join("real.sh");
        fs::write(&real, "old").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o750)).unwrap();
        let link = dir.join("link.sh");
        symlink(&real, &link).unwrap();

        write_text(&text(&link), "new", None).unwrap();

        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(fs::read_to_string(&real).unwrap(), "new");
        let mode = fs::metadata(&real).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o750);
    }

    #[test]
    fn reports_which_paths_are_folders() {
        let dir = scratch("kinds");
        let file = dir.join("a.txt");
        fs::write(&file, "x").unwrap();
        let missing = dir.join("missing");

        assert_eq!(
            paths_are_folders(vec![text(&dir), text(&file), text(&missing)]),
            vec![true, false, false]
        );
    }
}
