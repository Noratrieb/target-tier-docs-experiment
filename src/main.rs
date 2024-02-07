mod parse;

use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

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

fn main() {
    let rustc =
        PathBuf::from(std::env::var("RUSTC").expect("must pass RUSTC env var pointing to rustc"));

    let targets = rustc_stdout(&rustc, &["--print", "target-list"]);
    let targets = targets.lines().collect::<Vec<_>>();

    match std::fs::create_dir("targets/src") {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        e @ _ => e.unwrap(),
    }

    let mut info_patterns = parse::load_target_infos(Path::new("target_info"))
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

        std::fs::write(format!("targets/src/{target}.md"), doc).unwrap();
    }

    for target_pattern in info_patterns {
        if !target_pattern.used {
            panic!(
                "target pattern `{}` was never used",
                target_pattern.info.pattern
            );
        }
    }

    render_static(&targets);

    eprintln!("Finished generating target docs");
}

/// Renders a single target markdown file from the information obtained.
fn render_target_md(target: &TargetDocs, rustc_info: &RustcTargetInfo) -> String {
    let mut doc = format!("# {}\n**Tier: {}**", target.name, target.tier);

    let maintainers_str = if target.maintainers.is_empty() {
        "\n## Maintainers\nThis target does not have any maintainers!\n".to_owned()
    } else {
        format!(
            "\n## Maintainers\nThis target is maintained by:\n{}\n",
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
            Some((name, value)) => format!("## {name}\n{value}\n"),
            None => format!("## {section_name}\nUnknown.\n"),
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

/// Renders the non-target files like `SUMMARY.md` that depend on the target.
fn render_static(targets: &[&str]) {
    std::fs::write(
        format!("targets/src/SUMMARY.md"),
        format!(
            "\
# All targets
- [Info About This Thing](./information.md)
{}
",
            targets
                .iter()
                .map(|target| format!("- [{0}](./{0}.md)", target))
                .collect::<Vec<_>>()
                .join("\n")
        ),
    )
    .unwrap();
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

    // TODO: Render the nice table showing off all targets and their tier.
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
