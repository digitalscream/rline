//! Cline-compatible agent skills — lazy, description-matched instruction packs.
//!
//! A *skill* is a directory containing a `SKILL.md` file with YAML frontmatter
//! declaring a `name` and a `description`. Only the metadata is injected into
//! the agent's system prompt; the body is loaded on demand when the model
//! calls the `use_skill` tool.
//!
//! # Discovery roots
//!
//! Project-local (relative to the workspace root):
//! - `.cline/skills/`
//! - `.clinerules/skills/`
//! - `.claude/skills/`
//! - `.agents/skills/`
//!
//! Global (shared across workspaces):
//! - `~/.cline/skills/`
//! - `~/.agents/skills/`
//!
//! When the same skill name is defined in both project and global locations,
//! the global definition wins (matching Cline's precedence rule). A warning
//! is logged via [`tracing::warn!`] so the user can resolve the conflict.

pub mod error;
mod frontmatter;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::warn;

pub use error::SkillError;

/// Whether a skill came from a project-local or global discovery root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    /// Discovered under the workspace root (e.g. `.cline/skills/`).
    Project,
    /// Discovered under the user's home directory (e.g. `~/.cline/skills/`).
    Global,
}

/// Metadata for a discovered skill.
///
/// This is the lightweight form that is safe to inject into the system prompt
/// for every turn. The full body is only read on demand via [`load_skill_body`].
#[derive(Debug, Clone)]
pub struct SkillMetadata {
    /// The skill's name (matches its containing directory name).
    pub name: String,
    /// The one-line description from the frontmatter, used by the model to
    /// decide when to invoke the skill.
    pub description: String,
    /// Absolute path to the `SKILL.md` file.
    pub path: PathBuf,
    /// Whether the skill came from a project or global root.
    pub source: SkillSource,
}

/// Discover all skills reachable from the given workspace root.
///
/// Returns a deduplicated list (global wins on name collision). Malformed
/// skills are logged and skipped rather than failing discovery as a whole.
pub fn discover_skills(workspace_root: &Path) -> Vec<SkillMetadata> {
    let project_roots: Vec<PathBuf> = project_skill_roots()
        .iter()
        .map(|rel| workspace_root.join(rel))
        .collect();
    discover_from_roots(&project_roots, &global_skill_roots())
}

/// Discover skills from the given explicit project and global roots.
///
/// This variant is used by tests to avoid scanning the real user's home
/// directory. Production callers should use [`discover_skills`].
pub(crate) fn discover_from_roots(
    project_roots: &[PathBuf],
    global_roots: &[PathBuf],
) -> Vec<SkillMetadata> {
    let mut by_name: HashMap<String, SkillMetadata> = HashMap::new();

    // Project roots first — they will be overridden by globals on conflict.
    for dir in project_roots {
        scan_root(dir, SkillSource::Project, &mut by_name);
    }

    for dir in global_roots {
        scan_root(dir, SkillSource::Global, &mut by_name);
    }

    let mut skills: Vec<SkillMetadata> = by_name.into_values().collect();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    skills
}

/// Read and return the body (markdown content after the frontmatter) of a skill.
pub fn load_skill_body(meta: &SkillMetadata) -> Result<String, SkillError> {
    let text = std::fs::read_to_string(&meta.path).map_err(|source| SkillError::Io {
        path: meta.path.clone(),
        source,
    })?;
    let parsed = frontmatter::parse(&text, &meta.path)?;
    Ok(parsed.body)
}

/// Render the `## Skills` section for the system prompt, or `None` if no
/// skills were discovered.
pub fn format_skills_section(skills: &[SkillMetadata]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }

    let mut out = String::from(
        "## Skills\n\n\
         The following skills provide specialized instructions for specific tasks. \
         When a user's request matches a skill description, use the `use_skill` tool \
         to load and activate the skill.\n\n\
         Available skills:\n",
    );

    for s in skills {
        let desc = s.description.replace('\n', " ");
        out.push_str(&format!("  - \"{}\": {}\n", s.name, desc.trim()));
    }

    out.push_str(
        "\nTo use a skill:\n\
         1. Match the user's request to a skill based on its description.\n\
         2. Call `use_skill` with `skill_name` set to the exact skill name.\n\
         3. Follow the instructions returned by the tool.\n\
         4. Do NOT call `use_skill` again for the same skill within a single task.",
    );

    Some(out)
}

fn project_skill_roots() -> &'static [&'static str] {
    &[
        ".cline/skills",
        ".clinerules/skills",
        ".claude/skills",
        ".agents/skills",
    ]
}

fn global_skill_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(base) = directories::BaseDirs::new() {
        let home = base.home_dir();
        roots.push(home.join(".cline").join("skills"));
        roots.push(home.join(".agents").join("skills"));
    }
    roots
}

fn scan_root(root: &Path, source: SkillSource, out: &mut HashMap<String, SkillMetadata>) {
    if !root.is_dir() {
        return;
    }

    let entries = match std::fs::read_dir(root) {
        Ok(e) => e,
        Err(err) => {
            warn!("failed to read skill root {}: {err}", root.display());
            return;
        }
    };

    for entry in entries.filter_map(Result::ok) {
        let dir_path = entry.path();
        if !dir_path.is_dir() {
            continue;
        }
        let skill_md = dir_path.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }

        let dir_name = match dir_path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_owned(),
            None => continue,
        };

        match load_metadata(&skill_md, &dir_name, source) {
            Ok(meta) => {
                let existing_source = out.get(&meta.name).map(|m| m.source);
                match (existing_source, source) {
                    (None, _) => {
                        out.insert(meta.name.clone(), meta);
                    }
                    (Some(SkillSource::Project), SkillSource::Global) => {
                        warn!(
                            "skill '{}' defined in both project and global roots; using global ({})",
                            meta.name,
                            meta.path.display()
                        );
                        out.insert(meta.name.clone(), meta);
                    }
                    (Some(SkillSource::Global), SkillSource::Global)
                    | (Some(SkillSource::Project), SkillSource::Project) => {
                        warn!(
                            "duplicate skill '{}' found at {}; keeping earlier definition",
                            meta.name,
                            meta.path.display()
                        );
                    }
                    (Some(SkillSource::Global), SkillSource::Project) => {
                        // Globals already loaded; skip.
                    }
                }
            }
            Err(err) => {
                warn!("skipping malformed skill at {}: {err}", skill_md.display());
            }
        }
    }
}

fn load_metadata(
    skill_md: &Path,
    dir_name: &str,
    source: SkillSource,
) -> Result<SkillMetadata, SkillError> {
    let text = std::fs::read_to_string(skill_md).map_err(|source| SkillError::Io {
        path: skill_md.to_path_buf(),
        source,
    })?;
    let parsed = frontmatter::parse(&text, skill_md)?;

    if parsed.frontmatter.name != dir_name {
        return Err(SkillError::NameMismatch {
            name: parsed.frontmatter.name,
            dir_name: dir_name.to_owned(),
            path: skill_md.to_path_buf(),
        });
    }

    Ok(SkillMetadata {
        name: parsed.frontmatter.name,
        description: parsed.frontmatter.description,
        path: skill_md.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write_skill(dir: &Path, name: &str, description: &str, body: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).expect("create dir");
        let content = format!("---\nname: {name}\ndescription: {description}\n---\n{body}");
        fs::write(skill_dir.join("SKILL.md"), content).expect("write SKILL.md");
    }

    fn discover_isolated(project_root: &Path) -> Vec<SkillMetadata> {
        let project_roots: Vec<PathBuf> = project_skill_roots()
            .iter()
            .map(|rel| project_root.join(rel))
            .collect();
        discover_from_roots(&project_roots, &[])
    }

    #[test]
    fn test_discover_skills_project_only() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let skills_root = tmp.path().join(".cline").join("skills");
        fs::create_dir_all(&skills_root).expect("mkdir");
        write_skill(
            &skills_root,
            "hello-world",
            "Say hello.",
            "# Hello\n\nGreet.",
        );

        let skills = discover_isolated(tmp.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "hello-world");
        assert_eq!(skills[0].description, "Say hello.");
        assert_eq!(skills[0].source, SkillSource::Project);
    }

    #[test]
    fn test_discover_skills_finds_all_project_roots() {
        let tmp = tempfile::tempdir().expect("temp dir");

        for rel in [
            ".cline/skills",
            ".clinerules/skills",
            ".claude/skills",
            ".agents/skills",
        ] {
            let root = tmp.path().join(rel);
            fs::create_dir_all(&root).expect("mkdir");
            let unique = rel.replace('/', "-");
            write_skill(&root, &unique, "desc", "body");
        }

        let skills = discover_isolated(tmp.path());
        assert_eq!(skills.len(), 4, "expected one skill per root");
    }

    #[test]
    fn test_discover_skills_name_mismatch_skipped() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let skills_root = tmp.path().join(".cline").join("skills");
        fs::create_dir_all(&skills_root).expect("mkdir");

        let skill_dir = skills_root.join("dirname");
        fs::create_dir_all(&skill_dir).expect("mkdir");
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: different-name\ndescription: foo\n---\nbody",
        )
        .expect("write");

        let skills = discover_isolated(tmp.path());
        assert!(skills.is_empty(), "mismatched skill should be skipped");
    }

    #[test]
    fn test_discover_skills_missing_skill_md_skipped() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let skills_root = tmp.path().join(".cline").join("skills");
        fs::create_dir_all(skills_root.join("empty-dir")).expect("mkdir");

        let skills = discover_isolated(tmp.path());
        assert!(skills.is_empty());
    }

    #[test]
    fn test_discover_skills_malformed_skipped_others_kept() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path().join(".cline").join("skills");
        fs::create_dir_all(&root).expect("mkdir");

        write_skill(&root, "good", "good skill", "body");

        let bad_dir = root.join("bad");
        fs::create_dir_all(&bad_dir).expect("mkdir");
        fs::write(bad_dir.join("SKILL.md"), "no frontmatter here").expect("write");

        let skills = discover_isolated(tmp.path());
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "good");
    }

    #[test]
    fn test_discover_skills_global_overrides_project() {
        let project = tempfile::tempdir().expect("tmp");
        let global = tempfile::tempdir().expect("tmp");

        let proj_skills = project.path().join(".cline").join("skills");
        fs::create_dir_all(&proj_skills).expect("mkdir");
        write_skill(&proj_skills, "shared", "from project", "project body");

        let glob_skills = global.path().join("skills");
        fs::create_dir_all(&glob_skills).expect("mkdir");
        write_skill(&glob_skills, "shared", "from global", "global body");

        let project_roots = vec![proj_skills];
        let global_roots = vec![glob_skills];
        let skills = discover_from_roots(&project_roots, &global_roots);

        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].description, "from global");
        assert_eq!(skills[0].source, SkillSource::Global);
    }

    #[test]
    fn test_load_skill_body_returns_body_without_frontmatter() {
        let tmp = tempfile::tempdir().expect("temp dir");
        let root = tmp.path().join(".cline").join("skills");
        fs::create_dir_all(&root).expect("mkdir");
        write_skill(&root, "my-skill", "desc", "# Body heading\n\nContent.");

        let skills = discover_isolated(tmp.path());
        let body = load_skill_body(&skills[0]).expect("load body");
        assert!(body.starts_with("# Body heading"));
        assert!(!body.contains("name: my-skill"));
    }

    #[test]
    fn test_format_skills_section_empty_returns_none() {
        assert!(format_skills_section(&[]).is_none());
    }

    #[test]
    fn test_format_skills_section_lists_skills() {
        let skills = vec![SkillMetadata {
            name: "foo".into(),
            description: "Do foo things.".into(),
            path: PathBuf::from("/x/SKILL.md"),
            source: SkillSource::Project,
        }];
        let out = format_skills_section(&skills).expect("section");
        assert!(out.contains("## Skills"));
        assert!(out.contains("\"foo\": Do foo things."));
        assert!(out.contains("use_skill"));
    }

    #[test]
    fn test_format_skills_section_flattens_multiline_description() {
        let skills = vec![SkillMetadata {
            name: "foo".into(),
            description: "line one\nline two".into(),
            path: PathBuf::from("/x/SKILL.md"),
            source: SkillSource::Project,
        }];
        let out = format_skills_section(&skills).expect("section");
        assert!(out.contains("\"foo\": line one line two"));
    }
}
