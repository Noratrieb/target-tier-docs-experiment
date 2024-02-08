//! Suboptimal half-markdown parser that's just good-enough for this.

use eyre::{bail, OptionExt, Result, WrapErr};
use serde::Deserialize;
use std::{fs::DirEntry, path::Path};

#[derive(Debug, PartialEq, Clone, Deserialize)]
pub enum Tier {
    #[serde(rename = "1")]
    One,
    #[serde(rename = "2")]
    Two,
    #[serde(rename = "3")]
    Three,
}

#[derive(Debug)]
pub struct ParsedTargetInfoFile {
    pub pattern: String,
    pub tier: Option<Tier>,
    pub maintainers: Vec<String>,
    pub sections: Vec<(String, String)>,
    pub metadata: Vec<ParsedTargetMetadata>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frontmatter {
    tier: Option<Tier>,
    #[serde(default)]
    maintainers: Vec<String>,
    #[serde(default)]
    metadata: Vec<ParsedTargetMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParsedTargetMetadata {
    pub pattern: String,
    pub notes: String,
    pub std: TriStateBool,
    pub host: TriStateBool,
    #[serde(default)]
    pub footnotes: Vec<Footnote>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Footnote {
    pub name: String,
    pub content: String,
}

#[derive(Debug, PartialEq, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriStateBool {
    True,
    False,
    Unknown,
}

pub fn load_target_infos(directory: &Path) -> Result<Vec<ParsedTargetInfoFile>> {
    let dir = std::fs::read_dir(directory).unwrap();
    let mut infos = Vec::new();

    for entry in dir {
        let entry = entry?;
        infos.push(
            load_single_target_info(&entry)
                .wrap_err_with(|| format!("loading {}", entry.path().display()))?,
        )
    }

    Ok(infos)
}

fn load_single_target_info(entry: &DirEntry) -> Result<ParsedTargetInfoFile> {
    let pattern = entry.file_name();
    let name = pattern
        .to_str()
        .ok_or_eyre("file name is invalid utf8")?
        .strip_suffix(".md")
        .ok_or_eyre("target_info files must end with .md")?;
    let content: String = std::fs::read_to_string(entry.path()).wrap_err("reading content")?;

    parse_file(name, &content)
}

fn parse_file(name: &str, content: &str) -> Result<ParsedTargetInfoFile> {
    let mut frontmatter_splitter = content.split("---\n");

    let frontmatter = frontmatter_splitter
        .nth(1)
        .ok_or_eyre("missing frontmatter")?;

    let frontmatter_line_count = frontmatter.lines().count() + 2; // 2 from ---

    let mut frontmatter =
        serde_yaml::from_str::<Frontmatter>(frontmatter).wrap_err("invalid frontmatter")?;

    frontmatter.metadata.iter_mut().for_each(|meta| {
        meta.footnotes.iter_mut().for_each(|footnote| {
            footnote.content = footnote.content.replace("\r\n", " ").replace("\n", " ")
        })
    });
    let frontmatter = frontmatter;

    let body = frontmatter_splitter.next().ok_or_eyre("no body")?;

    let mut sections = Vec::<(String, String)>::new();
    let mut in_codeblock = false;

    for (idx, line) in body.lines().enumerate() {
        let number = frontmatter_line_count + idx + 1; // 1 because "line numbers" are off by 1
        if line.starts_with("```") {
            in_codeblock ^= true; // toggle
        } else if line.starts_with("#") {
            if in_codeblock {
                match sections.last_mut() {
                    Some((_, content)) => {
                        content.push_str(line);
                        content.push('\n');
                    }
                    None if line.trim().is_empty() => {}
                    None => {
                        bail!("line {number} with content not allowed before the first heading")
                    }
                }
            } else if let Some(header) = line.strip_prefix("## ") {
                if !crate::SECTIONS.contains(&header) {
                    bail!(
                        "on line {number}, `{header}` is not an allowed section name, must be one of {:?}",
                        super::SECTIONS
                    );
                }
                sections.push((header.to_owned(), String::new()));
            } else {
                bail!("on line {number}, the only allowed headings are `## `: `{line}`");
            }
        } else {
            match sections.last_mut() {
                Some((_, content)) => {
                    content.push_str(line);
                    content.push('\n');
                }
                None if line.trim().is_empty() => {}
                None => bail!("line with content not allowed before the first heading"),
            }
        }
    }

    sections
        .iter_mut()
        .for_each(|section| section.1 = section.1.trim().to_owned());

    Ok(ParsedTargetInfoFile {
        pattern: name.to_owned(),
        maintainers: frontmatter.maintainers,
        tier: frontmatter.tier,
        sections,
        metadata: frontmatter.metadata,
    })
}

#[cfg(test)]
mod tests {
    use crate::parse::Tier;

    #[test]
    fn no_frontmatter() {
        let name = "archlinux-unknown-linux-gnu.md"; // arch linux is an arch, right?
        let content = "";
        assert!(super::parse_file(name, content).is_err());
    }

    #[test]
    fn invalid_section() {
        let name = "6502-nintendo-nes.md";
        let content = "
---
---

## Not A Real Section
";

        assert!(super::parse_file(name, content).is_err());
    }

    #[test]
    fn wrong_header() {
        let name = "x86_64-known-linux-gnu.md";
        let content = "
---
---

# x86_64-known-linux-gnu
";

        assert!(super::parse_file(name, content).is_err());
    }

    #[test]
    fn parse_correctly() {
        let name = "cat-unknown-linux-gnu.md";
        let content = r#"
---
tier: "1" # first-class cats
maintainers: ["who maintains the cat?"]
---
## Requirements

This target mostly just meows and doesn't do much.

## Testing

You can pet the cat and it might respond positively.

## Cross compilation

If you're on a dog system, there might be conflicts with the cat, be careful.
But it should be possible.
        "#;

        let info = super::parse_file(name, content).unwrap();

        assert_eq!(info.maintainers, vec!["who maintains the cat?"]);
        assert_eq!(info.pattern, name);
        assert_eq!(info.tier, Some(Tier::One));
        assert_eq!(
            info.sections,
            vec![
                (
                    "Requirements".to_owned(),
                    "This target mostly just meows and doesn't do much.".to_owned(),
                ),
                (
                    "Testing".to_owned(),
                    "You can pet the cat and it might respond positively.".to_owned(),
                ),
                (
                    "Cross compilation".to_owned(),
                    "If you're on a dog system, there might be conflicts with the cat, be careful.\nBut it should be possible.".to_owned(),
                ),
            ]
        );
    }
}
