//! Tool: load and activate an on-demand skill's instructions.
//!
//! Skills are discovered by [`crate::skills::discover_skills`]. Only their
//! names and descriptions appear in the system prompt; the full body is
//! returned from this tool when the model decides a skill is relevant.

use std::path::Path;

use serde::Deserialize;

use crate::chat::types::ToolDefinition;
use crate::error::AiError;
use crate::skills;
use crate::tools::{Tool, ToolCategory, ToolResult};

/// Activate a discovered skill by returning its full `SKILL.md` body.
pub struct UseSkillTool;

#[derive(Debug, Deserialize)]
struct Args {
    skill_name: String,
}

impl Tool for UseSkillTool {
    fn name(&self) -> &str {
        "use_skill"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(
            "use_skill",
            "Load and activate a skill's full instructions. Call this when a user's request matches a skill listed in the Skills section of the system prompt. The returned text contains the instructions you must follow for that task. Do NOT call use_skill again for the same skill within a single task.",
            super::definitions::schema! {
                required: ["skill_name"],
                properties: {
                    "skill_name" => serde_json::json!({
                        "type": "string",
                        "description": "The exact name of the skill to activate (must match a name shown in the Skills section)."
                    })
                }
            },
        )
    }

    fn execute(&self, arguments: &str, workspace_root: &Path) -> Result<ToolResult, AiError> {
        let args: Args = serde_json::from_str(arguments)?;

        let available = skills::discover_skills(workspace_root);
        let meta = match available.iter().find(|s| s.name == args.skill_name) {
            Some(m) => m,
            None => {
                let names: Vec<&str> = available.iter().map(|s| s.name.as_str()).collect();
                let suffix = if names.is_empty() {
                    "No skills are currently available.".to_owned()
                } else {
                    format!("Available skills: {}.", names.join(", "))
                };
                return Ok(ToolResult::err(format!(
                    "Skill '{}' not found. {suffix}",
                    args.skill_name
                )));
            }
        };

        match skills::load_skill_body(meta) {
            Ok(body) => Ok(ToolResult::ok(format!(
                "Skill '{}' is now active. Follow these instructions for the current task, and do not call use_skill again for this skill.\n\n{body}",
                meta.name
            ))),
            Err(err) => Ok(ToolResult::err(format!(
                "Failed to load skill '{}': {err}",
                meta.name
            ))),
        }
    }

    fn is_read_only(&self) -> bool {
        true
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::ReadFile
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_workspace(skill_name: &str, description: &str, body: &str) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().expect("temp dir");
        let dir = tmp.path().join(".cline").join("skills").join(skill_name);
        fs::create_dir_all(&dir).expect("mkdir");
        let content = format!("---\nname: {skill_name}\ndescription: {description}\n---\n{body}");
        fs::write(dir.join("SKILL.md"), content).expect("write");
        tmp
    }

    #[test]
    fn test_use_skill_returns_body() {
        let tmp = make_workspace(
            "rline-test-use-skill",
            "A test skill.",
            "# Instructions\n\nDo the thing.",
        );
        let tool = UseSkillTool;
        let args = serde_json::json!({ "skill_name": "rline-test-use-skill" }).to_string();
        let result = tool.execute(&args, tmp.path()).expect("exec");
        assert!(result.success);
        assert!(result.output.contains("# Instructions"));
        assert!(result.output.contains("Do the thing."));
        assert!(result.output.contains("rline-test-use-skill"));
    }

    #[test]
    fn test_use_skill_unknown_name_lists_available() {
        let tmp = make_workspace("rline-test-known", "Known.", "Body.");
        let tool = UseSkillTool;
        let args = serde_json::json!({ "skill_name": "does-not-exist" }).to_string();
        let result = tool.execute(&args, tmp.path()).expect("exec");
        assert!(!result.success);
        assert!(result.output.contains("'does-not-exist' not found"));
        assert!(result.output.contains("rline-test-known"));
    }

    #[test]
    fn test_use_skill_category_and_flags() {
        let tool = UseSkillTool;
        assert!(tool.is_read_only());
        assert_eq!(tool.category(), ToolCategory::ReadFile);
        assert_eq!(tool.name(), "use_skill");
    }
}
