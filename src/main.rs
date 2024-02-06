fn main() {
    let targets = include_str!("../targets.txt").lines().collect::<Vec<_>>();

    for target in &targets {
        let doc = format!("# {target}\nthis is a target.");
        std::fs::write(format!("targets/src/{target}.md"), doc).unwrap();
    }

    std::fs::write(
        format!("targets/src/SUMMARY.md"),
        format!(
            "\
# All targets
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
    println!("generated some target docs :3");
}
