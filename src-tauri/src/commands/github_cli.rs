//! Repository and branch suggestions using the user's authenticated GitHub CLI.

use crate::agents::process::find_bin;
use serde::Serialize;
use std::process::{Command, Stdio};

const GH_MISSING: &str =
    "GitHub CLI (`gh`) not found. Install it and run `gh auth login`, then retry.";

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubCliRepos {
    /// Repository detected from the workflow working directory, if any.
    pub current: Option<String>,
    pub repos: Vec<String>,
    pub error: Option<String>,
}

/// `owner/name` only. The value reaches an API path, so reject anything else.
pub fn valid_repo(repo: &str) -> bool {
    let mut parts = repo.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let ok = |segment: &str| {
        !segment.is_empty()
            && segment.len() <= 100
            && segment
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    };
    ok(owner) && ok(name)
}

pub(crate) fn gh(
    cwd: Option<&str>,
    args: &[impl AsRef<std::ffi::OsStr>],
) -> Result<String, String> {
    let bin = find_bin("gh").ok_or_else(|| GH_MISSING.to_string())?;
    let mut cmd = Command::new(bin);
    if let Some(dir) = cwd.map(str::trim).filter(|dir| !dir.is_empty()) {
        cmd.current_dir(dir);
    }
    let out = cmd
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("CLICOLOR", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("gh failed to start: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "gh exited with an error".to_string()
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

fn repos_blocking(cwd: Option<String>) -> GithubCliRepos {
    let cwd = cwd.as_deref();
    let mut result = GithubCliRepos::default();

    if cwd.is_some_and(|dir| !dir.trim().is_empty()) {
        result.current = gh(
            cwd,
            &[
                "repo",
                "view",
                "--json",
                "nameWithOwner",
                "--jq",
                ".nameWithOwner",
            ],
        )
        .ok()
        .filter(|repo| valid_repo(repo));
    }

    match gh(
        None,
        &[
            "api",
            "-X",
            "GET",
            "user/repos",
            "-f",
            "per_page=100",
            "-f",
            "sort=updated",
            "-f",
            "affiliation=owner,collaborator,organization_member",
            "--jq",
            ".[].full_name",
        ],
    ) {
        Ok(text) => {
            let mut repos = Vec::new();
            for repo in result.current.iter().cloned().chain(lines(&text)) {
                if valid_repo(&repo) && !repos.contains(&repo) {
                    repos.push(repo);
                }
            }
            result.repos = repos;
        }
        Err(e) => result.error = Some(e),
    }
    result
}

fn branches_blocking(repo: String) -> Result<Vec<String>, String> {
    let repo = repo.trim();
    if !valid_repo(repo) {
        return Err("Repository must look like `owner/name`.".to_string());
    }

    gh(
        None,
        &[
            "api",
            "-X",
            "GET",
            &format!("repos/{repo}/branches"),
            "-f",
            "per_page=100",
            "--jq",
            ".[].name",
        ],
    )
    .map(|text| lines(&text))
}

#[tauri::command]
pub async fn github_cli_repos(cwd: Option<String>) -> GithubCliRepos {
    tauri::async_runtime::spawn_blocking(move || repos_blocking(cwd))
        .await
        .unwrap_or_else(|e| GithubCliRepos {
            error: Some(format!("gh lookup failed: {e}")),
            ..Default::default()
        })
}

#[tauri::command]
pub async fn github_cli_branches(repo: String) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || branches_blocking(repo))
        .await
        .map_err(|e| format!("gh lookup failed: {e}"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_names_outside_owner_slash_name_are_rejected() {
        assert!(valid_repo("owner/name"));
        assert!(valid_repo("my-org/my.repo_2"));
        assert!(!valid_repo("owner"));
        assert!(!valid_repo("owner/name/extra"));
        assert!(!valid_repo("owner/"));
        assert!(!valid_repo("../../etc"));
        assert!(!valid_repo("owner/name?per_page=1"));
        assert!(!valid_repo(""));
    }

    #[test]
    fn lines_drops_blank_padding() {
        assert_eq!(lines("a\n\n  b \n"), vec!["a".to_string(), "b".to_string()]);
    }
}
