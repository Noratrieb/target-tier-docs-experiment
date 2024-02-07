mod parse;

use std::{
    fs, io,
    path::{Path, PathBuf},
    process::Command,
};

use eyre::{bail, Context, OptionExt, Result};
use parse::ParsedTargetInfoFile;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

/// Information about a target obtained from `target_info.toml``.
struct TargetDocs {
    name: String,
    maintainers: Vec<String>,
    sections: Vec<(String, String)>,
    tier: String,
}

const SECTIONS: &[&str] = &[
    "Requirements",
    "Testing",
    "Building",
    "Cross compilation",
    "Building Rust programs",
];

fn is_in_rust_lang_rust() -> bool {
    std::env::var("RUST_LANG_RUST") == Ok("1".to_owned())
}

fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let input_dir = args
        .get(1)
        .ok_or_eyre("first argument must be path to directory containing source md files")?;
    let output_src = args
        .get(2)
        .ok_or_eyre("second argument must be path to `src` output directory")?;

    let rustc =
        PathBuf::from(std::env::var("RUSTC").expect("must pass RUSTC env var pointing to rustc"));

    let targets = rustc_stdout(&rustc, &["--print", "target-list"]);
    let targets = targets.lines().collect::<Vec<_>>();

    if !is_in_rust_lang_rust() {
        match std::fs::create_dir("targets/src") {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            e @ _ => e.unwrap(),
        }
    }

    let mut info_patterns = parse::load_target_infos(Path::new(input_dir))
        .unwrap()
        .into_iter()
        .map(|info| TargetPatternEntry { info, used: false })
        .collect::<Vec<_>>();

    eprintln!("Collecting rustc information");
    let rustc_infos = targets
        .par_iter()
        .map(|target| rustc_target_info(&rustc, target))
        .collect::<Vec<_>>();

    eprintln!("Rendering targets");
    for (target, rustc_info) in std::iter::zip(&targets, rustc_infos) {
        let info = target_info(&mut info_patterns, target);
        let doc = render_target_md(&info, &rustc_info);

        std::fs::write(
            Path::new(output_src)
                .join("platform-support")
                .join("targets")
                .join(format!("{target}.md")),
            doc,
        )
        .wrap_err("writing target file")?;
    }

    for target_pattern in info_patterns {
        if !target_pattern.used {
            bail!(
                "target pattern `{}` was never used",
                target_pattern.info.pattern
            );
        }
    }

    render_static(&Path::new(output_src).join("platform-support"), &targets)?;

    eprintln!("Finished generating target docs");
    Ok(())
}

/// Renders a single target markdown file from the information obtained.
fn render_target_md(target: &TargetDocs, rustc_info: &RustcTargetInfo) -> String {
    let mut doc = format!("# {}\n\n**Tier: {}**\n\n", target.name, target.tier);

    doc.push_str("## Maintainers\n");

    let maintainers_str = if target.maintainers.is_empty() {
        "This target does not have any maintainers!\n".to_owned()
    } else {
        format!(
            "This target is maintained by:\n{}\n",
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

    doc.push_str(&maintainers_str);

    for section_name in SECTIONS {
        let value = target
            .sections
            .iter()
            .find(|(name, _)| name == section_name);

        let section_str = match value {
            Some((name, value)) => format!("## {name}\n{value}\n\n"),
            None => format!("## {section_name}\nUnknown.\n\n"),
        };
        doc.push_str(&section_str)
    }

    let cfg_text = rustc_info
        .target_cfgs
        .iter()
        .map(|(key, value)| format!("- `{key}` = `{value}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let cfg_text =
        format!("This target defines the following target-specific cfg values:\n{cfg_text}\n");

    doc.push_str(&format!("## cfg\n{cfg_text}\n"));

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
fn render_static(platform_support: &Path, targets: &[&str]) -> Result<()> {
    let targets_file = platform_support.join("targets.md");
    let old_targets = fs::read_to_string(&targets_file).wrap_err("reading summary file")?;

    let target_list = targets
        .iter()
        .map(|target| format!("- [{0}](targets/{0}.md)", target))
        .collect::<Vec<_>>()
        .join("\n");

    let new_targets =
        replace_section(&old_targets, "TARGET", &target_list).wrap_err("replacing targets.md")?;

    std::fs::write(targets_file, new_targets).wrap_err("writing targets.md")?;

    if !is_in_rust_lang_rust() {
        std::fs::write(
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
        .unwrap();
    }

    // TODO: Render the nice table showing off all targets and their tier.

    Ok(())
}

struct TargetPatternEntry {
    info: ParsedTargetInfoFile,
    used: bool,
}

/// Gets the target information from `target_info.toml` by applying all patterns that match.
fn target_info(info_patterns: &mut [TargetPatternEntry], target: &str) -> TargetDocs {
    let mut tier = None;
    let mut maintainers = Vec::new();
    let mut sections = Vec::new();

    for target_pattern in info_patterns {
        if glob_match::glob_match(&target_pattern.info.pattern, target) {
            target_pattern.used = true;
            let target_pattern = &target_pattern.info;

            maintainers.extend_from_slice(&target_pattern.maintainers);

            if let Some(pattern_value) = &target_pattern.tier {
                if tier.is_some() {
                    panic!("target {target} inherits a tier from multiple patterns, create a more specific pattern and add it there");
                }
                tier = Some(pattern_value.clone());
            }

            for (section_name, content) in &target_pattern.sections {
                if sections.iter().any(|(name, _)| name == section_name) {
                    panic!("target {target} inherits the section {section_name} from multiple patterns, create a more specific pattern and add it there");
                }
                sections.push((section_name.clone(), content.clone()));
            }
        }
    }

    TargetDocs {
        name: target.to_owned(),
        maintainers,
        // tier: tier.expect(&format!("no tier found for target {target}")),
        tier: tier.unwrap_or("UNKNOWN".to_owned()),
        sections,
    }
}

/// Information about a target obtained from rustc.
struct RustcTargetInfo {
    target_cfgs: Vec<(String, String)>,
}

/// Get information about a target from rustc.
fn rustc_target_info(rustc: &Path, target: &str) -> RustcTargetInfo {
    let cfgs = rustc_stdout(rustc, &["--print", "cfg", "--target", target]);
    let target_cfgs = cfgs
        .lines()
        .filter_map(|line| {
            if line.starts_with("target_") {
                let Some((key, value)) = line.split_once("=") else {
                    // For example `unix`
                    return None;
                };
                Some((key.to_owned(), value.to_owned()))
            } else {
                None
            }
        })
        .collect();
    RustcTargetInfo { target_cfgs }
}

fn rustc_stdout(rustc: &Path, args: &[&str]) -> String {
    let output = Command::new(rustc).args(args).output().unwrap();
    if !output.status.success() {
        panic!(
            "rustc failed: {}, {}",
            output.status,
            String::from_utf8(output.stderr).unwrap_or_default()
        )
    }
    String::from_utf8(output.stdout).unwrap()
}
