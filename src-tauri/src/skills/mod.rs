//! Skill discovery and prompt wiring.
//!
//! Skills are `SKILL.md` packages (Claude Code / Cursor style). An agent step
//! can optionally pin a specific skill; adapters then invoke it (e.g. `/skill-name`).

use crate::agents::AgentProvider;
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRef {
    /// Slash-command / folder name, e.g. `tdd` → invoked as `/tdd`.
    pub name: String,
    pub description: String,
    pub path: String,
    /// Where it was found: project or user.
    pub source: SkillSource,
    /// Agent-branded directory it came from. `.agents/skills` stays shared.
    #[serde(default)]
    pub source_agent: Option<String>,
    /// Providers that can consume this skill format today.
    pub providers: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    Project,
    User,
}

#[derive(Debug, Error)]
pub enum SkillError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Build the prompt string an adapter should send when skills are selected.
/// Claude Code / Cursor-style: `/skill-a /skill-b` plus the user prompt.
pub fn compose_prompt_with_skills(skill_names: &[&str], prompt: &str) -> String {
    let mut seen = std::collections::HashSet::<String>::new();
    let skills: Vec<String> = skill_names
        .iter()
        .map(|s| s.trim().trim_start_matches('/').to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| seen.insert(s.clone()))
        .collect();

    let prompt = prompt.trim();
    if skills.is_empty() {
        return prompt.to_string();
    }

    let prefix = skills
        .iter()
        .map(|s| format!("/{s}"))
        .collect::<Vec<_>>()
        .join(" ");

    if prompt.is_empty() {
        prefix
    } else if prompt.starts_with('/') {
        // Already a slash invocation — leave as-is (user owns the prompt).
        prompt.to_string()
    } else {
        format!("{prefix} {prompt}")
    }
}

/// Single-skill helper (keeps call sites / tests concise).
pub fn compose_prompt_with_skill(skill_name: &str, prompt: &str) -> String {
    compose_prompt_with_skills(&[skill_name], prompt)
}

pub fn list_skills(project_root: Option<&str>) -> Result<Vec<SkillRef>, SkillError> {
    let mut skills = Vec::new();
    let mut seen = std::collections::HashSet::<String>::new();

    if let Some(root) = project_root {
        let root = PathBuf::from(root);
        for dir in project_skill_dirs(&root) {
            collect_skills_from_dir(
                &dir.path,
                SkillSource::Project,
                dir.source_agent,
                &mut skills,
                &mut seen,
            )?;
        }
    }

    if let Some(base) = BaseDirs::new() {
        for dir in user_skill_dirs(base.home_dir()) {
            collect_skills_from_dir(
                &dir.path,
                SkillSource::User,
                dir.source_agent,
                &mut skills,
                &mut seen,
            )?;
        }
    }

    skills.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(skills)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SkillDirectory {
    path: PathBuf,
    source_agent: Option<AgentProvider>,
}

fn skill_dir(path: PathBuf, source_agent: Option<AgentProvider>) -> SkillDirectory {
    SkillDirectory { path, source_agent }
}

fn project_skill_dirs(root: &Path) -> Vec<SkillDirectory> {
    vec![
        skill_dir(root.join(".claude/skills"), Some(AgentProvider::ClaudeCode)),
        skill_dir(root.join(".cursor/skills"), Some(AgentProvider::Cursor)),
        skill_dir(
            root.join(".github/skills"),
            Some(AgentProvider::GithubCopilot),
        ),
        skill_dir(root.join(".gemini/skills"), Some(AgentProvider::Gemini)),
        skill_dir(root.join(".grok/skills"), Some(AgentProvider::Grok)),
        skill_dir(root.join(".pi/skills"), Some(AgentProvider::Pi)),
        skill_dir(root.join(".omp/skills"), Some(AgentProvider::Omp)),
        skill_dir(root.join(".agents/skills"), None),
        skill_dir(root.join(".opencode/skills"), Some(AgentProvider::Opencode)),
    ]
}

fn user_skill_dirs(home: &Path) -> Vec<SkillDirectory> {
    vec![
        skill_dir(home.join(".claude/skills"), Some(AgentProvider::ClaudeCode)),
        skill_dir(home.join(".cursor/skills"), Some(AgentProvider::Cursor)),
        skill_dir(
            home.join(".copilot/skills"),
            Some(AgentProvider::GithubCopilot),
        ),
        skill_dir(home.join(".gemini/skills"), Some(AgentProvider::Gemini)),
        skill_dir(home.join(".grok/skills"), Some(AgentProvider::Grok)),
        skill_dir(home.join(".pi/agent/skills"), Some(AgentProvider::Pi)),
        skill_dir(home.join(".omp/agent/skills"), Some(AgentProvider::Omp)),
        skill_dir(
            home.join(".cursor/skills-cursor"),
            Some(AgentProvider::Cursor),
        ),
        skill_dir(home.join(".agents/skills"), None),
        skill_dir(home.join(".codex/skills"), Some(AgentProvider::Codex)),
        skill_dir(home.join(".opencode/skills"), Some(AgentProvider::Opencode)),
    ]
}

fn collect_skills_from_dir(
    dir: &Path,
    source: SkillSource,
    source_agent: Option<AgentProvider>,
    out: &mut Vec<SkillRef>,
    seen: &mut std::collections::HashSet<String>,
) -> Result<(), SkillError> {
    if !dir.is_dir() {
        return Ok(());
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let skill_md = path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }

        let folder_name = entry.file_name().to_string_lossy().to_string();
        let content = fs::read_to_string(&skill_md)?;
        let meta = parse_frontmatter(&content);

        let name = meta
            .name
            .unwrap_or(folder_name)
            .trim()
            .trim_start_matches('/')
            .to_string();

        if name.is_empty() || !seen.insert(name.clone()) {
            continue;
        }

        out.push(SkillRef {
            name,
            description: meta.description.unwrap_or_default(),
            path: skill_md.to_string_lossy().to_string(),
            source,
            source_agent: source_agent.map(|provider| provider.as_str().to_string()),
            // SKILL.md is the shared format across these CLIs for now.
            providers: vec![
                AgentProvider::ClaudeCode.as_str().to_string(),
                AgentProvider::Cursor.as_str().to_string(),
                AgentProvider::Codex.as_str().to_string(),
                AgentProvider::Opencode.as_str().to_string(),
                AgentProvider::GithubCopilot.as_str().to_string(),
                AgentProvider::Gemini.as_str().to_string(),
                AgentProvider::Grok.as_str().to_string(),
                AgentProvider::Pi.as_str().to_string(),
                AgentProvider::Omp.as_str().to_string(),
            ],
        });
    }

    Ok(())
}

#[derive(Default)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
}

fn parse_frontmatter(content: &str) -> Frontmatter {
    let mut meta = Frontmatter::default();
    let mut lines = content.lines();
    if lines.next().map(str::trim) != Some("---") {
        return meta;
    }
    let body: Vec<&str> = lines.take_while(|line| line.trim() != "---").collect();
    let mut index = 0;
    while index < body.len() {
        let line = body[index];
        let trimmed = line.trim();
        if trimmed.is_empty() || line.starts_with(' ') || line.starts_with('\t') {
            index += 1;
            continue;
        }
        let Some((key, raw_value)) = trimmed.split_once(':') else {
            index += 1;
            continue;
        };
        let key = key.trim();
        let raw_value = raw_value.trim();
        let mut value = raw_value.to_string();
        if raw_value.starts_with('>') || raw_value.starts_with('|') {
            let literal = raw_value.starts_with('|');
            let mut parts = Vec::new();
            index += 1;
            while index < body.len() {
                let next = body[index];
                if !next.starts_with(' ') && !next.starts_with('\t') && !next.trim().is_empty() {
                    break;
                }
                parts.push(next.trim().to_string());
                index += 1;
            }
            value = if literal {
                parts.join("\n")
            } else {
                parts.join(" ")
            }
            .trim()
            .to_string();
        } else {
            value = raw_value.trim_matches('"').trim_matches('\'').to_string();
            index += 1;
        }
        match key {
            "name" => meta.name = Some(value),
            "description" => meta.description = Some(value),
            _ => {}
        }
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDir(PathBuf);

    impl TestDir {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("alfred-skill-test-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        fn skill_dir(&self, relative: &str, name: &str) -> PathBuf {
            let dir = self.0.join(relative);
            let skill = dir.join(name);
            fs::create_dir_all(&skill).expect("create skill directory");
            fs::write(
                skill.join("SKILL.md"),
                format!("---\nname: {name}\ndescription: Test skill\n---\n"),
            )
            .expect("write test skill");
            dir
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn compose_prepends_slash_skill() {
        assert_eq!(
            compose_prompt_with_skill("tdd", "add a failing test"),
            "/tdd add a failing test"
        );
        assert_eq!(compose_prompt_with_skill("/review", ""), "/review");
        assert_eq!(
            compose_prompt_with_skill("x", "/already there"),
            "/already there"
        );
    }

    #[test]
    fn compose_multiple_skills() {
        assert_eq!(
            compose_prompt_with_skills(&["tdd", "review"], "fix the flaky test"),
            "/tdd /review fix the flaky test"
        );
        assert_eq!(
            compose_prompt_with_skills(&["tdd", "tdd", "/review"], ""),
            "/tdd /review"
        );
        assert_eq!(
            compose_prompt_with_skills(&[], "just the prompt"),
            "just the prompt"
        );
    }

    #[test]
    fn discovery_records_an_agent_specific_source() {
        let root = TestDir::new();
        let claude_project = root.skill_dir("project-claude", "review");
        let mut skills = Vec::new();
        let mut seen = std::collections::HashSet::new();

        collect_skills_from_dir(
            &claude_project,
            SkillSource::Project,
            Some(AgentProvider::ClaudeCode),
            &mut skills,
            &mut seen,
        )
        .unwrap();

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].source_agent.as_deref(), Some("claude_code"));
        assert_eq!(skills[0].source, SkillSource::Project);
        assert_eq!(
            skills[0].providers,
            vec![
                "claude_code",
                "cursor",
                "codex",
                "opencode",
                "github_copilot",
                "gemini",
                "grok",
                "pi",
                "omp"
            ]
        );
    }

    #[test]
    fn project_skill_directories_record_the_owning_agent() {
        let root = Path::new("/project");
        let directories = project_skill_dirs(root);
        let actual = directories
            .iter()
            .map(|entry| {
                (
                    entry.path.as_path(),
                    entry.source_agent.map(AgentProvider::as_str),
                )
            })
            .collect::<Vec<_>>();

        assert_eq!(
            actual,
            vec![
                (Path::new("/project/.claude/skills"), Some("claude_code")),
                (Path::new("/project/.cursor/skills"), Some("cursor")),
                (Path::new("/project/.github/skills"), Some("github_copilot")),
                (Path::new("/project/.gemini/skills"), Some("gemini")),
                (Path::new("/project/.grok/skills"), Some("grok")),
                (Path::new("/project/.pi/skills"), Some("pi")),
                (Path::new("/project/.omp/skills"), Some("omp")),
                (Path::new("/project/.agents/skills"), None),
                (Path::new("/project/.opencode/skills"), Some("opencode")),
            ]
        );
    }

    #[test]
    fn frontmatter_reads_folded_and_literal_descriptions() {
        let folded = parse_frontmatter(
            "---\nname: demo\ndescription: >\n  first line\n  second line\n---\nBody\n",
        );
        assert_eq!(folded.name.as_deref(), Some("demo"));
        assert_eq!(
            folded.description.as_deref(),
            Some("first line second line")
        );

        let literal = parse_frontmatter(
            "---\nname: demo\ndescription: |\n  first line\n  second line\n---\nBody\n",
        );
        assert_eq!(
            literal.description.as_deref(),
            Some("first line\nsecond line")
        );
    }
}
