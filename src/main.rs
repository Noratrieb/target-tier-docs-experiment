mod parse;
mod render;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Command,
};

use eyre::{bail, Context, OptionExt, Result};
use parse::ParsedTargetInfoFile;
use serde::Deserialize;

/// Information about a target obtained from the target_info markdown file.
struct TargetDocs {
    name: String,
    maintainers: Vec<String>,
    sections: Vec<(String, String)>,
    footnotes: Vec<String>,
}

/// All the sections that we want every doc page to have.
/// It may make sense to relax this into two kinds of sections, "required" sections
/// and "optional" sections, where required sections will get stubbed out when not found
/// while optional sections will just not exist when not found.
// IMPORTANT: This is also documented in the README, keep it in sync.
const SECTIONS: &[&str] = &[
    "Overview",
    "Requirements",
    "Testing",
    "Building the target",
    "Cross compilation",
    "Building Rust programs",
];

fn main() -> Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let input_dir = args
        .get(1)
        .ok_or_eyre("first argument must be path to target_infos directory containing target source md files (src/doc/rustc/target_infos/)")?;
    let output_src = args
        .get(2)
        .ok_or_eyre("second argument must be path to `src` output directory (src/doc/rustc/src)")?;

    let rustc =
        PathBuf::from(std::env::var("RUSTC").expect("must pass RUSTC env var pointing to rustc"));
    let check_only = std::env::var("TARGET_CHECK_ONLY") == Ok("1".to_owned());

    let targets = rustc_stdout(&rustc, &["--print", "target-list"]);
    let targets = targets.lines().collect::<Vec<_>>();

    let mut info_patterns = parse::load_target_infos(Path::new(input_dir))
        .wrap_err("failed loading target_info")?
        .into_iter()
        .map(|info| {
            let footnotes_used = info
                .footnotes
                .iter()
                .map(|(target, _)| (target.clone(), false))
                .collect();
            TargetPatternEntry {
                info,
                used: false,
                footnotes_used,
            }
        })
        .collect::<Vec<_>>();

    eprintln!("Collecting rustc information");
    let rustc_infos = targets
        .iter()
        .map(|target| rustc_target_info(&rustc, target))
        .collect::<Vec<_>>();

    let targets = targets
        .into_iter()
        .map(|target| target_doc_info(&mut info_patterns, target))
        .zip(rustc_infos)
        .collect::<Vec<_>>();

    eprintln!("Rendering targets check_only={check_only}");
    let targets_dir = Path::new(output_src)
        .join("platform-support")
        .join("targets");
    if !check_only {
        std::fs::create_dir_all(&targets_dir).wrap_err("creating platform-support/targets dir")?;
    }
    for (info, rustc_info) in &targets {
        let doc = render::render_target_md(info, rustc_info);

        if !check_only {
            std::fs::write(targets_dir.join(format!("{}.md", info.name)), doc)
                .wrap_err("writing target file")?;
        }
    }

    for target_pattern in info_patterns {
        if !target_pattern.used {
            bail!(
                "target pattern `{}` was never used",
                target_pattern.info.pattern
            );
        }

        for footnote_target in target_pattern.info.footnotes.keys() {
            let used = target_pattern.footnotes_used[footnote_target];
            if !used {
                bail!(
                    "in target pattern `{}`, the footnotes for target `{}` were never used",
                    target_pattern.info.pattern,
                    footnote_target,
                );
            }
        }
    }

    render::render_static(check_only, Path::new(output_src), &targets)?;

    eprintln!("Finished generating target docs");
    Ok(())
}

struct TargetPatternEntry {
    info: ParsedTargetInfoFile,
    used: bool,
    footnotes_used: HashMap<String, bool>,
}

fn target_doc_info(info_patterns: &mut [TargetPatternEntry], target: &str) -> TargetDocs {
    let mut tier = None;
    let mut maintainers = Vec::new();
    let mut sections = Vec::new();

    let mut footnotes = Vec::new();

    for target_pattern_entry in info_patterns {
        if glob_match::glob_match(&target_pattern_entry.info.pattern, target) {
            target_pattern_entry.used = true;
            let target_pattern = &target_pattern_entry.info;

            maintainers.extend_from_slice(&target_pattern.maintainers);

            if let Some(pattern_value) = &target_pattern.tier {
                if tier.is_some() {
                    panic!(
                        "target {target} inherits a tier from multiple patterns, create a more specific pattern and add it there"
                    );
                }
                tier = Some(pattern_value.clone());
            }

            for (section_name, content) in &target_pattern.sections {
                if sections.iter().any(|(name, _)| name == section_name) {
                    panic!(
                        "target {target} inherits the section {section_name} from multiple patterns, create a more specific pattern and add it there"
                    );
                }
                sections.push((section_name.clone(), content.clone()));
            }

            if let Some(target_footnotes) = target_pattern.footnotes.get(target) {
                target_pattern_entry
                    .footnotes_used
                    .insert(target.to_owned(), true);

                if !footnotes.is_empty() {
                    panic!("target {target} is assigned metadata from more than one pattern");
                }
                footnotes = target_footnotes.clone();
            }
        }
    }

    TargetDocs {
        name: target.to_owned(),
        maintainers,
        sections,
        footnotes,
    }
}

/// Information about a target obtained from rustc.
struct RustcTargetInfo {
    target_cfgs: Vec<(String, String)>,
    metadata: RustcTargetMetadata,
}

#[derive(Deserialize)]
struct RustcTargetMetadata {
    description: Option<String>,
    tier: Option<u8>,
    host_tools: Option<bool>,
    std: Option<bool>,
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

    #[derive(Deserialize)]
    struct TargetJson {
        metadata: RustcTargetMetadata,
    }

    let json_spec = rustc_stdout(
        rustc,
        &[
            "-Zunstable-options",
            "--print",
            "target-spec-json",
            "--target",
            target,
        ],
    );
    let spec = serde_json::from_str::<TargetJson>(&json_spec)
        .expect("parsing --print target-spec-json for metadata");

    RustcTargetInfo {
        target_cfgs,
        metadata: spec.metadata,
    }
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
