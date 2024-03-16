# target-docs

This tool generates target documentation for all targets in the rustc book.

To achieve this, it uses a list of input markdown files provided in `src/doc/rustc/target_infos`. These files follow a strict format.
Every file covers a glob pattern of targets according to its file name.

For every rustc target, we iterate through all the target infos and find matching globs.
When a glob matches, it extracts the h2 markdown sections and saves them for the target.

In the end, a page is generated for every target using these sections.
Sections that are not provided are stubbed out. Currently, the sections are

- Overview
- Requirements
- Testing
- Building the target
- Cross compilation
- Building Rust programs

In addition to the markdown sections, we also have extra data about the targets.
This is achieved through YAML frontmatter.

The frontmatter follows the following format:

```yaml
tier: "1"
maintainers: ["@someone"]
metadata:
    - target: "i686-pc-windows-gnu"
      notes: "32-bit MinGW (Windows 7+)"
      std: true
      host: true
      footnotes:
        - name: "x86_32-floats-return-ABI"
          content: |
            Due to limitations of the C ABI, floating-point support on `i686` targets is non-compliant:
            floating-point return values are passed via an x87 register, so NaN payload bits can be lost.
            See [issue #114479][https://github.com/rust-lang/rust/issues/114479].
        - name: "windows-support"
          content: "Only Windows 10 currently undergoes automated testing. Earlier versions of Windows rely on testing and support from the community."
```

The top level keys are:

- `tier` (optional): `1`, `2` or `3`
- `maintainers` (optional): list of strings

There is also `metadata`, which is specific to every single target and not just a target "group" (the glob).

`metadata` has the following properties:

- `target`: the target name
- `notes`: a string containing a short description of the target for the table
- `std`: `true`, `false`, `unknown`, whether the target has `std`
- `host`: `true`, `false`, `unknown`, whether the target has host tools
- `footnotes` (optional): a list of footnotes, where every footnote has a `name` and `content`. These are used in the table.
