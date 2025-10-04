use std::{
    collections::HashMap,
    fmt::Write as _,
    path::{Path, PathBuf},
};

fn parse_bool(value: Option<&String>) -> bool {
    value
        .map(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "True"))
        .unwrap_or(false)
}

fn clean_string(value: Option<&String>) -> Option<String> {
    value
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
}

fn normalize_root(value: Option<&String>) -> String {
    value
        .map(|v| v.trim().trim_matches('/'))
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .unwrap_or_default()
}

fn join_path(path: &str, root: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    let base = Path::new(path);
    let combined: PathBuf = if root.is_empty() {
        base.to_path_buf()
    } else {
        base.join(root)
    };

    Some(combined.to_string_lossy().to_string())
}

#[derive(Debug, Clone)]
struct RepoSummary {
    prefix: String,
    name: Option<String>,
    path: Option<String>,
    root: String,
    branch: Option<String>,
    base_branch: Option<String>,
    is_primary: bool,
    effective_dir: Option<String>,
}

impl RepoSummary {
    fn from_prefix(prefix: &str, env: &HashMap<String, String>) -> Option<Self> {
        let key = |suffix: &str| format!("VIBE_REPO_{}_{}", prefix, suffix);

        let path = clean_string(env.get(&key("PATH")));
        let root = normalize_root(env.get(&key("ROOT")));
        let branch = clean_string(env.get(&key("BRANCH")));
        let base_branch = clean_string(env.get(&key("BASE_BRANCH")));
        let name = clean_string(env.get(&key("NAME")));
        let is_primary = parse_bool(env.get(&key("IS_PRIMARY")));
        let effective_dir = path.as_deref().and_then(|p| join_path(p, &root));

        Some(Self {
            prefix: prefix.to_string(),
            name,
            path,
            root,
            branch,
            base_branch,
            is_primary,
            effective_dir,
        })
    }

    fn root_display(&self) -> &str {
        if self.root.is_empty() {
            "/"
        } else {
            &self.root
        }
    }

    fn branch_display(&self) -> &str {
        self.branch
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("<not yet created>")
    }

    fn base_branch_display(&self) -> &str {
        self.base_branch
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("<inherit project target branch>")
    }

    fn name_display(&self) -> &str {
        self.name
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("(unnamed repository)")
    }

    fn effective_dir_display(&self) -> &str {
        self.effective_dir
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or("<path unavailable>")
    }
}

fn collect_prefixes(env: &HashMap<String, String>) -> Vec<String> {
    if let Some(list_raw) = env.get("VIBE_REPOSITORIES") {
        let prefixes = list_raw
            .split(',')
            .filter_map(|entry| {
                let trimmed = entry.trim();
                (!trimmed.is_empty()).then_some(trimmed.to_string())
            })
            .collect::<Vec<_>>();

        if !prefixes.is_empty() {
            return prefixes;
        }
    }

    let mut prefixes = env
        .keys()
        .filter_map(|key| {
            key.strip_prefix("VIBE_REPO_")
                .and_then(|rest| rest.strip_suffix("_PATH"))
                .map(|prefix| prefix.to_string())
        })
        .collect::<Vec<_>>();
    prefixes.sort();
    prefixes.dedup();
    prefixes
}

fn build_repository_summaries(env: &HashMap<String, String>) -> Vec<RepoSummary> {
    collect_prefixes(env)
        .into_iter()
        .filter_map(|prefix| RepoSummary::from_prefix(&prefix, env))
        .collect()
}

fn format_repository_instructions(env: &HashMap<String, String>) -> Option<String> {
    let mut repos = build_repository_summaries(env);
    if repos.is_empty() {
        return None;
    }

    let repo_count = env
        .get("VIBE_REPOSITORY_COUNT")
        .and_then(|val| val.trim().parse::<usize>().ok())
        .unwrap_or(repos.len());

    let primary_prefix_env = clean_string(env.get("VIBE_PRIMARY_REPO_PREFIX"));
    if let Some(prefix) = primary_prefix_env.as_deref() {
        repos.sort_by(|a, b| match (a.prefix == prefix, b.prefix == prefix) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.prefix.cmp(&b.prefix),
        });
    } else {
        repos.sort_by(|a, b| a.prefix.cmp(&b.prefix));
    }

    let mut instructions = String::new();
    instructions.push_str("## Repository Context\n");

    let prefix_list = repos
        .iter()
        .map(|repo| repo.prefix.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let _ = writeln!(
        instructions,
        "- Repositories available: {} ({})",
        repo_count, prefix_list
    );

    if let Some(primary) = repos.iter().find(|repo| repo.is_primary) {
        let _ = writeln!(
            instructions,
            "- Primary repository: `{}` — {} (workdir: `{}`)",
            primary.prefix,
            primary.name_display(),
            primary.effective_dir_display()
        );
    } else if let Some(first) = repos.first() {
        let _ = writeln!(
            instructions,
            "- No explicit primary repo flagged; default to `{}` (workdir: `{}`) unless instructed otherwise",
            first.prefix,
            first.effective_dir_display()
        );
    }

    instructions.push_str(
        "- Always set `workdir` in commands/tools to the repo you are touching (see effective directories below).\n",
    );
    instructions.push_str(
        "- When referencing files, prefix paths with the repo name if they are outside the primary repo.\n",
    );
    instructions.push_str(
        "- Use the `VIBE_REPO_<PREFIX>_*` variables for automation; `VIBE_PRIMARY_REPO_*` mirrors the current primary.\n",
    );

    for repo in repos {
        let primary_label = if repo.is_primary { " (primary)" } else { "" };
        let _ = writeln!(
            instructions,
            "\n- `{}` — {}{}",
            repo.prefix,
            repo.name_display(),
            primary_label
        );

        if let Some(path) = &repo.path {
            let _ = writeln!(instructions, "  - Path: `{}`", path);
        } else {
            instructions.push_str("  - Path: <unavailable>\n");
        }

        let _ = writeln!(
            instructions,
            "  - Root: `{}` (effective workdir: `{}`)",
            repo.root_display(),
            repo.effective_dir_display()
        );

        let _ = writeln!(
            instructions,
            "  - Branch: `{}` (base: `{}`)",
            repo.branch_display(),
            repo.base_branch_display()
        );

        let _ = writeln!(
            instructions,
            "  - Env vars: `VIBE_REPO_{prefix}_PATH`, `VIBE_REPO_{prefix}_ROOT`, `VIBE_REPO_{prefix}_BRANCH`, `VIBE_REPO_{prefix}_BASE_BRANCH`, `VIBE_REPO_{prefix}_NAME`, `VIBE_REPO_{prefix}_IS_PRIMARY`",
            prefix = repo.prefix
        );

        if !repo.root.is_empty() {
            instructions.push_str(
                "  - Note: repo is scoped to a subdirectory; operate relative to the effective workdir.\n",
            );
        }
    }

    Some(instructions)
}

/// Append repository-aware instructions to the prompt when multi-repo metadata is available.
pub fn augment_prompt_with_repo_context(
    prompt: &str,
    env: Option<&HashMap<String, String>>,
) -> String {
    if let Some(env) = env {
        if let Some(repo_instructions) = format_repository_instructions(env) {
            let mut combined = String::with_capacity(prompt.len() + repo_instructions.len() + 2);
            combined.push_str(prompt);
            if !prompt.ends_with('\n') {
                combined.push_str("\n\n");
            } else {
                combined.push('\n');
            }
            combined.push_str(&repo_instructions);
            return combined;
        }
    }

    prompt.to_string()
}

#[cfg(test)]
mod tests {
    use super::augment_prompt_with_repo_context;
    use std::collections::HashMap;

    fn mock_env() -> HashMap<String, String> {
        let mut env = HashMap::new();
        env.insert("VIBE_REPOSITORY_COUNT".into(), "2".into());
        env.insert("VIBE_REPOSITORIES".into(), "WEB,API".into());
        env.insert("VIBE_PRIMARY_REPO_PREFIX".into(), "WEB".into());

        env.insert("VIBE_REPO_WEB_PATH".into(), "/work/web-app".into());
        env.insert("VIBE_REPO_WEB_ROOT".into(), "frontend".into());
        env.insert("VIBE_REPO_WEB_BRANCH".into(), "feature/web-ui".into());
        env.insert("VIBE_REPO_WEB_BASE_BRANCH".into(), "main".into());
        env.insert("VIBE_REPO_WEB_NAME".into(), "Web Client".into());
        env.insert("VIBE_REPO_WEB_IS_PRIMARY".into(), "1".into());

        env.insert("VIBE_REPO_API_PATH".into(), "/work/core-api".into());
        env.insert("VIBE_REPO_API_ROOT".into(), "".into());
        env.insert("VIBE_REPO_API_BRANCH".into(), "feature/api".into());
        env.insert("VIBE_REPO_API_BASE_BRANCH".into(), "develop".into());
        env.insert("VIBE_REPO_API_NAME".into(), "Core API".into());
        env.insert("VIBE_REPO_API_IS_PRIMARY".into(), "0".into());

        env
    }

    #[test]
    fn appends_repository_context_when_env_present() {
        let env = mock_env();
        let prompt = "Implement feature";
        let augmented = augment_prompt_with_repo_context(prompt, Some(&env));

        assert!(augmented.contains("## Repository Context"));
        assert!(augmented.contains("Web Client"));
        assert!(augmented.contains("Core API"));
        assert!(augmented.contains("VIBE_REPO_WEB_PATH"));
        assert!(augmented.contains("VIBE_REPO_API_IS_PRIMARY"));
    }

    #[test]
    fn leaves_prompt_unchanged_without_env() {
        let prompt = "Just do it";
        let augmented = augment_prompt_with_repo_context(prompt, None);
        assert_eq!(augmented, prompt);
    }
}
