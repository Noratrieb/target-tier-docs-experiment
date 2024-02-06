use std::io;

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

fn render_target_md(target: &TargetDocs) -> String {
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

    doc
}

fn main() {
    let targets = include_str!("../targets.txt").lines().collect::<Vec<_>>();

    match std::fs::create_dir("targets/src") {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        e @ _ => e.unwrap(),
    }

    for target in &targets {
        let doc = render_target_md(&target_info(target));

        std::fs::write(format!("targets/src/{target}.md"), doc).unwrap();
    }

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
    std::fs::write("targets/src/information.md", "\
# platform support generated

This is an experiment of what target tier documentation could look like.

See https://github.com/Nilstrieb/target-tier-docs-experiment for the source.
    ").unwrap();
    println!("generated some target docs :3");
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetMaintainerTable {
    target: Vec<TargetMaintainerEntry>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetMaintainerEntry {
    pattern: String,
    tier: Option<u8>,
    requirements: Option<String>,
    testing: Option<String>,
    building_the_target: Option<String>,
    cross_compilation: Option<String>,
    building_rust_programs: Option<String>,
    maintainers: Vec<String>,
}

fn target_info(target: &str) -> TargetDocs {
    let file = include_str!("../target_info.toml");
    let table = toml::from_str::<TargetMaintainerTable>(file).unwrap();

    let mut tier = None;
    let mut maintainers = Vec::new();
    let mut requirements = None;
    let mut testing = None;
    let mut building_the_target = None;
    let mut cross_compilation = None;
    let mut building_rust_programs = None;

    for target_pattern in table.target {
        if glob_match::glob_match(&target_pattern.pattern, target) {
            maintainers.extend_from_slice(&target_pattern.maintainers);

            fn set_once<T>(
                target: &str,
                pattern_value: Option<T>,
                to_insert: &mut Option<T>,
                name: &str,
            ) {
                if let Some(pattern_value) = pattern_value {
                    if to_insert.is_some() {
                        panic!("target {target} inherits a {name} from multiple patterns, create a more specific pattern and add it there");
                    }
                    *to_insert = Some(pattern_value);
                }
            }
            #[rustfmt::skip]
            {
                set_once(target, target_pattern.tier, &mut tier, "tier");
                set_once(target, target_pattern.requirements, &mut requirements, "requirements");
                set_once(target, target_pattern.testing, &mut testing, "testing");
                set_once(target, target_pattern.building_the_target, &mut building_the_target, "building_the_target");
                set_once(target, target_pattern.cross_compilation, &mut cross_compilation, "cross_compilation");
                set_once(target, target_pattern.building_rust_programs, &mut building_rust_programs, "building_rust_programs");
            };
        }
    }

    // we should give errors for unused patterns.

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
