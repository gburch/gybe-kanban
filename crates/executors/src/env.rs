use std::collections::HashMap;

use tokio::process::Command;

pub fn apply_env(command: &mut Command, env: Option<&HashMap<String, String>>) {
    if let Some(entries) = env {
        for (key, value) in entries {
            command.env(key, value);
        }
    }
}
