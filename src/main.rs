use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

/// Information about a target obtained from `target_info.toml``.
struct TargetDocs {
    name: String,
    maintainers: Vec<String>,
    requirements: Option<String>,
    testing: Option<String>,
    building_the_target: Option<String>,
    cross_compilation: Option<String>,
    building_rust_programs: Option<String>,
    tier: u8,
}

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

    let mut info_patterns = load_target_info_patterns();

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

    let mut section = |value: &Option<String>, name| {
        let section_str = match value {
            Some(value) => format!("## {name}\n{value}\n"),
            None => format!("## {name}\nUnknown.\n"),
        };
        doc.push_str(&section_str)
    };

    section(&target.requirements, "Requirements");
    section(&target.testing, "Testing");
    section(&target.building_the_target, "Building");
    section(&target.cross_compilation, "Cross Compilation");
    section(&target.building_rust_programs, "Building Rust Programs");

    let cfg_text = rustc_info
        .target_cfgs
        .iter()
        .map(|(key, value)| format!("- `{key}` = `{value}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let cfg_text =
        format!("This target defines the following target-specific cfg values:\n{cfg_text}\n");
    section(&Some(cfg_text), "cfg");

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

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetInfoTable {
    target: Vec<TargetInfoPattern>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetInfoPattern {
    pattern: String,
    #[serde(default)]
    maintainers: Vec<String>,
    tier: Option<u8>,
    requirements: Option<String>,
    testing: Option<String>,
    building_the_target: Option<String>,
    cross_compilation: Option<String>,
    building_rust_programs: Option<String>,
}

struct TargetPatternEntry {
    info: TargetInfoPattern,
    used: bool,
}

fn load_target_info_patterns() -> Vec<TargetPatternEntry> {
    let file = include_str!("../target_info.toml");
    let table = toml::from_str::<TargetInfoTable>(file).unwrap();

    table
        .target
        .into_iter()
        .map(|info| TargetPatternEntry { info, used: false })
        .collect()
}

/// Gets the target information from `target_info.toml` by applying all patterns that match.
fn target_info(info_patterns: &mut [TargetPatternEntry], target: &str) -> TargetDocs {
    let mut tier = None;
    let mut maintainers = Vec::new();
    let mut requirements = None;
    let mut testing = None;
    let mut building_the_target = None;
    let mut cross_compilation = None;
    let mut building_rust_programs = None;

    for target_pattern in info_patterns {
        if glob_match::glob_match(&target_pattern.info.pattern, target) {
            target_pattern.used = true;
            let target_pattern = &target_pattern.info;

            maintainers.extend_from_slice(&target_pattern.maintainers);

            fn set_once<T: Clone>(
                target: &str,
                pattern_value: &Option<T>,
                to_insert: &mut Option<T>,
                name: &str,
            ) {
                if let Some(pattern_value) = pattern_value {
                    if to_insert.is_some() {
                        panic!("target {target} inherits a {name} from multiple patterns, create a more specific pattern and add it there");
                    }
                    *to_insert = Some(pattern_value.clone());
                }
            }
            #[rustfmt::skip]
            {
                set_once(target, &target_pattern.tier, &mut tier, "tier");
                set_once(target, &target_pattern.requirements, &mut requirements, "requirements");
                set_once(target, &target_pattern.testing, &mut testing, "testing");
                set_once(target, &target_pattern.building_the_target, &mut building_the_target, "building_the_target");
                set_once(target, &target_pattern.cross_compilation, &mut cross_compilation, "cross_compilation");
                set_once(target, &target_pattern.building_rust_programs, &mut building_rust_programs, "building_rust_programs");
            };
        }
    }

    TargetDocs {
        name: target.to_owned(),
        maintainers,
        requirements,
        testing,
        building_the_target,
        cross_compilation,
        building_rust_programs,
        // tier: tier.expect(&format!("no tier found for target {target}")),
        tier: tier.unwrap_or(0),
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
