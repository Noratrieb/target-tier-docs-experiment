use eyre::{Context, OptionExt, Result};
use std::{fs, path::Path};

use crate::{
    is_in_rust_lang_rust,
    parse::{Tier, TriStateBool},
    RustcTargetInfo, TargetDocs,
};

impl TargetDocs {
    fn has_host_tools(&self) -> bool {
        self.metadata
            .as_ref()
            .map_or(false, |meta| meta.host == TriStateBool::True)
    }
}

/// Renders a single target markdown file from the information obtained.
pub fn render_target_md(target: &TargetDocs, rustc_info: &RustcTargetInfo) -> String {
    let mut doc = format!(
        "# {}\n\n**Tier: {}**\n\n",
        target.name,
        match target.tier {
            Some(Tier::One) => "1",
            Some(Tier::Two) => "2",
            Some(Tier::Three) => "3",
            None => "UNKNOWN",
        }
    );

    let mut section = |name: &str, content: &str| {
        doc.push_str("## ");
        doc.push_str(name.trim());
        doc.push('\n');
        doc.push_str(content.trim());
        doc.push_str("\n\n");
    };

    let maintainers_content = if target.maintainers.is_empty() {
        "This target does not have any maintainers!".to_owned()
    } else {
        format!(
            "This target is maintained by:\n{}",
            target
                .maintainers
                .iter()
                .map(|maintainer| {
                    let maintainer = if maintainer.starts_with('@') && !maintainer.contains(" ") {
                        format!(
                            "[@{0}](https://github.com/{0})",
                            maintainer.strip_prefix("@").unwrap()
                        )
                    } else {
                        maintainer.to_owned()
                    };

                    format!("- {maintainer}")
                })
                .collect::<Vec<_>>()
                .join("\n")
        )
    };

    section("Maintainers", &maintainers_content);

    for section_name in crate::SECTIONS {
        let value = target
            .sections
            .iter()
            .find(|(name, _)| name == section_name);

        let section_content = match value {
            Some((_, value)) => value.clone(),
            None => "Unknown.".to_owned(),
        };
        section(&section_name, &section_content);
    }

    let cfg_text = rustc_info
        .target_cfgs
        .iter()
        .map(|(key, value)| format!("- `{key}` = `{value}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let cfg_content =
        format!("This target defines the following target-specific cfg values:\n{cfg_text}\n");

    section("cfg", &cfg_content);

    doc
}

/// Replaces inner part of the form
/// `<!-- {section_name} SECTION START --><!-- {section_name} SECTION END -->`
/// with replacement`.
fn replace_section(prev_content: &str, section_name: &str, replacement: &str) -> Result<String> {
    let magic_summary_start = format!("<!-- {section_name} SECTION START -->");
    let magic_summary_end = format!("<!-- {section_name} SECTION END -->");

    let (pre_target, target_and_after) = prev_content
        .split_once(&magic_summary_start)
        .ok_or_eyre("<!-- TARGET SECTION START --> not found")?;

    let (_, post_target) = target_and_after
        .split_once(&magic_summary_end)
        .ok_or_eyre("<!-- TARGET SECTION START --> not found")?;

    let new = format!(
        "{pre_target}{magic_summary_start}\n{replacement}\n{magic_summary_end}{post_target}"
    );
    Ok(new)
}

/// Renders the non-target files like `SUMMARY.md` that depend on the target.
pub fn render_static(src_output: &Path, targets: &[(TargetDocs, RustcTargetInfo)]) -> Result<()> {
    let targets_file = src_output.join("platform-support").join("targets.md");
    let old_targets = fs::read_to_string(&targets_file).wrap_err("reading summary file")?;

    let target_list = targets
        .iter()
        .map(|(target, _)| format!("- [{0}](targets/{0}.md)", target.name))
        .collect::<Vec<_>>()
        .join("\n");

    let new_targets =
        replace_section(&old_targets, "TARGET", &target_list).wrap_err("replacing targets.md")?;

    fs::write(targets_file, new_targets).wrap_err("writing targets.md")?;

    if !is_in_rust_lang_rust() {
        fs::write(
            "targets/src/information.md",
            "\
    # platform support generated

    This is an experiment of what generated target tier documentation could look like.

    See <https://github.com/Nilstrieb/target-tier-docs-experiment> for the source.
    The README of the repo contains more information about the motivation and benefits.

    Targets of interest with information filled out are any tvos targets like [aarch64-apple-tvos](./aarch64-apple-tvos.md)
    and [powerpc64-ibm-aix](./powerpc64-ibm-aix.md).

    But as you might notice, all targets are actually present with a stub :3.
        ",
        )
        .wrap_err("writing front page information about experiment")?;
    }

    // TODO: Render the nice table showing off all targets and their tier.
    let platform_support_main = src_output.join("platform-support.md");
    let platform_support_main_old =
        fs::read_to_string(&platform_support_main).wrap_err("reading platform-support.md")?;

    // needs footnotes...
    //let tier1host_table = render_table(
    //    targets
    //        .into_iter()
    //        .filter(|target| target.0.tier == Some(Tier::One)),
    //)?;
    // Tier 2 without host doesn't exist right now
    // they all support std, obviously
    //let tier2host_table = render_table_without_std this needs a better scheme??(
    //    targets
    //        .into_iter()
    //        .filter(|target| target.0.tier == Some(Tier::Two) && target.0.has_host_tools()),
    //)
    //.wrap_err("rendering tier 2 table")?;
    let tier2_table = render_table(
        targets
            .into_iter()
            .filter(|target| target.0.tier == Some(Tier::Two) && !target.0.has_host_tools()),
    )?;
    let tier3_table = render_table_with_host(
        targets
            .into_iter()
            .filter(|target| target.0.tier == Some(Tier::Three)),
    )?;

    let content = platform_support_main_old;
    let content = replace_section(&content, "TIER2", &tier2_table)
        .wrap_err("replacing platform support.md")?;
    let content = replace_section(&content, "TIER3", &tier3_table)
        .wrap_err("replacing platform support.md")?;

    fs::write(platform_support_main, content).wrap_err("writing platform-support.md")?;

    Ok(())
}

fn render_table_tri_state_bool(bool: TriStateBool) -> &'static str {
    match bool {
        TriStateBool::True => "✓",
        TriStateBool::False => " ",
        TriStateBool::Unknown => "?",
    }
}

fn render_table_with_host<'a>(
    targets: impl IntoIterator<Item = &'a (TargetDocs, RustcTargetInfo)>,
) -> Result<String> {
    let mut rows = Vec::new();

    for (target, _) in targets {
        let meta = target.metadata.as_ref();
        let std = meta
            .map(|meta| render_table_tri_state_bool(meta.std))
            .unwrap_or("?");
        let host = meta
            .map(|meta| render_table_tri_state_bool(meta.host))
            .unwrap_or("?");

        let notes = meta.map(|meta| meta.notes.as_str()).unwrap_or("unknown");
        rows.push(format!(
            "[`{0}`](platform-support/targets/{0}.md) | {std} | {host} | {notes}",
            target.name
        ));
    }

    Ok(rows.join("\n"))
}

fn render_table<'a>(
    targets: impl IntoIterator<Item = &'a (TargetDocs, RustcTargetInfo)>,
) -> Result<String> {
    let mut rows = Vec::new();

    for (target, _) in targets {
        let meta = target.metadata.as_ref();
        let std = meta
            .map(|meta| render_table_tri_state_bool(meta.std))
            .unwrap_or("?");
        let notes = meta.map(|meta| meta.notes.as_str()).unwrap_or("unknown");
        rows.push(format!(
            "[`{0}`](platform-support/targets/{0}.md) | {std} | {notes}",
            target.name
        ));
    }

    Ok(rows.join("\n"))
}
