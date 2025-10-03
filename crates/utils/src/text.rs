use regex::Regex;
use uuid::Uuid;

pub fn git_branch_id(input: &str) -> String {
    // 1. lowercase
    let lower = input.to_lowercase();

    // 2. replace non-alphanumerics with hyphens
    let re = Regex::new(r"[^a-z0-9]+").unwrap();
    let slug = re.replace_all(&lower, "-");

    // 3. trim extra hyphens
    let trimmed = slug.trim_matches('-');

    // 4. take up to 16 chars, then trim trailing hyphens again
    let cut: String = trimmed.chars().take(16).collect();
    cut.trim_end_matches('-').to_string()
}

pub fn short_uuid(u: &Uuid) -> String {
    // to_simple() gives you a 32-char hex string with no hyphens
    let full = u.simple().to_string();
    full.chars().take(4).collect() // grab the first 4 chars
}

/// Produce a git branch name using the configured prefix, task title slug, and short attempt id.
pub fn git_branch_name_with_prefix(
    branch_prefix: &str,
    attempt_id: &Uuid,
    task_title: &str,
) -> String {
    let normalized_prefix = {
        let trimmed = branch_prefix.trim();
        if trimmed.is_empty() {
            String::new()
        } else if trimmed.ends_with('/') || trimmed.ends_with('-') || trimmed.ends_with('_') {
            trimmed.to_string()
        } else {
            format!("{trimmed}/")
        }
    };

    let short_id = short_uuid(attempt_id);
    let task_title_id = git_branch_id(task_title);

    if normalized_prefix.is_empty() {
        format!("{}-{}", short_id, task_title_id)
    } else {
        format!("{}{}-{}", normalized_prefix, short_id, task_title_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt_id() -> Uuid {
        Uuid::parse_str("12345678-1234-1234-1234-123456789abc").unwrap()
    }

    #[test]
    fn adds_separator_when_prefix_missing_one() {
        let branch = git_branch_name_with_prefix("greg", &attempt_id(), "My Feature!");

        assert_eq!(branch, "greg/1234-my-feature");
    }

    #[test]
    fn omits_prefix_when_empty_after_trim() {
        let branch = git_branch_name_with_prefix("   ", &attempt_id(), "My Feature!");

        assert_eq!(branch, "1234-my-feature");
    }
}
