mod parse;
mod render;

use std::{
    io,
    path::{Path, PathBuf},
    process::Command,
};

use eyre::{bail, Context, OptionExt, Result};
use parse::{ParsedTargetInfoFile, Tier, TriStateBool};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

/// Information about a target obtained from `target_info.toml``.
struct TargetDocs {
    name: String,
    maintainers: Vec<String>,
    sections: Vec<(String, String)>,
    tier: Option<Tier>,
    // TODO: Make this mandatory.
    metadata: Option<TargetMetadata>,
}

/// Metadata for the table
struct TargetMetadata {
    notes: String,
    std: TriStateBool,
    host: TriStateBool,
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
        .map(|info| {
            let metadata_used = vec![false; info.metadata.len()];
            TargetPatternEntry {
                info,
                used: false,
                metadata_used,
            }
        })
        .collect::<Vec<_>>();

    eprintln!("Collecting rustc information");
    let rustc_infos = targets
        .par_iter()
        .map(|target| rustc_target_info(&rustc, target))
        .collect::<Vec<_>>();

    let targets = targets
        .into_iter()
        .map(|target| target_doc_info(&mut info_patterns, target))
        .zip(rustc_infos)
        .collect::<Vec<_>>();

    eprintln!("Rendering targets");
    for (info, rustc_info) in &targets {
        let doc = render::render_target_md(info, rustc_info);

        std::fs::write(
            Path::new(output_src)
                .join("platform-support")
                .join("targets")
                .join(format!("{}.md", info.name)),
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

        for (used, meta) in
            std::iter::zip(target_pattern.metadata_used, target_pattern.info.metadata)
        {
            if !used {
                bail!(
                    "in target pattern `{}`, the metadata pattern `{}` was never used",
                    target_pattern.info.pattern,
                    meta.pattern
                );
            }
        }
    }

    render::render_static(Path::new(output_src), &targets)?;

    eprintln!("Finished generating target docs");
    Ok(())
}

struct TargetPatternEntry {
    info: ParsedTargetInfoFile,
    used: bool,
    metadata_used: Vec<bool>,
}

fn target_doc_info(info_patterns: &mut [TargetPatternEntry], target: &str) -> TargetDocs {
    let mut tier = None;
    let mut maintainers = Vec::new();
    let mut sections = Vec::new();

    let mut metadata = None;

    for target_pattern_entry in info_patterns {
        if glob_match::glob_match(&target_pattern_entry.info.pattern, target) {
            target_pattern_entry.used = true;
            let target_pattern = &target_pattern_entry.info;

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

            for (i, metadata_pattern) in target_pattern.metadata.iter().enumerate() {
                if glob_match::glob_match(&metadata_pattern.pattern, target) {
                    target_pattern_entry.metadata_used[i] = true;
                    if metadata.is_some() {
                        panic!("target {target} is assigned metadata from more than one pattern");
                    }
                    metadata = Some(TargetMetadata {
                        notes: metadata_pattern.notes.clone(),
                        host: metadata_pattern.host.clone(),
                        std: metadata_pattern.std.clone(),
                    });
                }
            }
        }
    }

    TargetDocs {
        name: target.to_owned(),
        maintainers,
        tier,
        sections,
        metadata,
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
