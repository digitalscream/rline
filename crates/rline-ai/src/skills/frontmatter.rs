//! YAML frontmatter parsing for `SKILL.md` files.

use std::path::Path;

use serde::Deserialize;

use super::error::SkillError;

/// The frontmatter fields required by a Cline-compatible skill.
#[derive(Debug, Deserialize)]
pub(crate) struct Frontmatter {
    pub name: String,
    pub description: String,
}

/// The result of splitting a `SKILL.md`: parsed frontmatter + body text.
#[derive(Debug)]
pub(crate) struct ParsedSkill {
    pub frontmatter: Frontmatter,
    pub body: String,
}

/// Parse a `SKILL.md` file's text into its YAML frontmatter and markdown body.
///
/// The file must begin with `---` on its first non-empty line, followed by
/// YAML, followed by a closing `---` line. Everything after the closing fence
/// is returned as the body (leading blank lines trimmed).
pub(crate) fn parse(text: &str, path: &Path) -> Result<ParsedSkill, SkillError> {
    let trimmed = text.trim_start_matches('\u{feff}');
    let mut iter = trimmed.split_inclusive('\n');

    let first = iter.next().ok_or_else(|| SkillError::MissingFrontmatter {
        path: path.to_path_buf(),
    })?;
    if first.trim_end_matches(['\r', '\n']).trim() != "---" {
        return Err(SkillError::MissingFrontmatter {
            path: path.to_path_buf(),
        });
    }

    let mut yaml = String::new();
    let mut body_start = first.len();
    let mut found_close = false;

    for line in iter {
        body_start += line.len();
        if line.trim_end_matches(['\r', '\n']).trim() == "---" {
            found_close = true;
            break;
        }
        yaml.push_str(line);
    }

    if !found_close {
        return Err(SkillError::MissingFrontmatter {
            path: path.to_path_buf(),
        });
    }

    let fm: Frontmatter = serde_yaml::from_str(&yaml).map_err(|source| SkillError::Yaml {
        path: path.to_path_buf(),
        source,
    })?;

    if fm.name.trim().is_empty() {
        return Err(SkillError::MissingField {
            field: "name",
            path: path.to_path_buf(),
        });
    }
    if fm.description.trim().is_empty() {
        return Err(SkillError::MissingField {
            field: "description",
            path: path.to_path_buf(),
        });
    }

    let body = trimmed[body_start..].trim_start_matches('\n').to_owned();

    Ok(ParsedSkill {
        frontmatter: fm,
        body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> std::path::PathBuf {
        std::path::PathBuf::from("/tmp/test/SKILL.md")
    }

    #[test]
    fn test_parse_frontmatter_valid() {
        let text = "---\nname: foo\ndescription: bar baz\n---\n# Heading\n\nBody.\n";
        let parsed = parse(text, &path()).expect("should parse");
        assert_eq!(parsed.frontmatter.name, "foo");
        assert_eq!(parsed.frontmatter.description, "bar baz");
        assert!(parsed.body.starts_with("# Heading"));
        assert!(parsed.body.contains("Body."));
    }

    #[test]
    fn test_parse_frontmatter_missing_name() {
        let text = "---\ndescription: only description\n---\nBody";
        let err = parse(text, &path()).expect_err("should fail");
        match err {
            SkillError::Yaml { .. } => {}
            SkillError::MissingField { field: "name", .. } => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_parse_frontmatter_empty_name() {
        let text = "---\nname: \"\"\ndescription: something\n---\nBody";
        let err = parse(text, &path()).expect_err("should fail");
        assert!(matches!(
            err,
            SkillError::MissingField { field: "name", .. }
        ));
    }

    #[test]
    fn test_parse_frontmatter_no_fence() {
        let text = "name: foo\ndescription: bar\nBody without fences";
        let err = parse(text, &path()).expect_err("should fail");
        assert!(matches!(err, SkillError::MissingFrontmatter { .. }));
    }

    #[test]
    fn test_parse_frontmatter_unclosed_fence() {
        let text = "---\nname: foo\ndescription: bar\nBody without close";
        let err = parse(text, &path()).expect_err("should fail");
        assert!(matches!(err, SkillError::MissingFrontmatter { .. }));
    }

    #[test]
    fn test_parse_frontmatter_bom_tolerated() {
        let text = "\u{feff}---\nname: foo\ndescription: bar\n---\nBody";
        let parsed = parse(text, &path()).expect("should parse");
        assert_eq!(parsed.frontmatter.name, "foo");
    }

    #[test]
    fn test_parse_frontmatter_multiline_description() {
        let text = "---\nname: foo\ndescription: |\n  line one\n  line two\n---\nBody";
        let parsed = parse(text, &path()).expect("should parse");
        assert!(parsed.frontmatter.description.contains("line one"));
        assert!(parsed.frontmatter.description.contains("line two"));
    }

    #[test]
    fn test_parse_frontmatter_crlf_line_endings() {
        let text = "---\r\nname: foo\r\ndescription: bar\r\n---\r\nBody line\r\n";
        let parsed = parse(text, &path()).expect("should parse");
        assert_eq!(parsed.frontmatter.name, "foo");
        assert!(parsed.body.contains("Body line"));
    }
}
