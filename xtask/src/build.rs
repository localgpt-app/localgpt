//! `cargo xtask build <target…> [--release] [-- <cargo args…>]`: LocalGPT's
//! binaries with cargo, by name (it was `bin/build/target.sh`). Binaries
//! land in `target/debug/`, or `target/release/` with `--release` (LTO and
//! stripped, so slow). Everything after `--` goes to cargo. The macOS
//! bundle is its own command, `cargo xtask bundle`.

use std::path::Path;
use std::process::Command;

pub const USAGE: &str = "  build <target…> [--release] [-- cargo args…]
                   by name: cli app gen previs md verse, or all";

/// A target's name and its cargo package.
const TARGETS: [(&str, &str); 6] = [
    ("cli", "localgpt"),
    ("app", "localgpt-app"),
    ("gen", "localgpt-gen"),
    ("previs", "localgpt-previs"),
    ("md", "localgpt-md"),
    ("verse", "localgpt-verse"),
];

/// The cargo arguments the targets ask for.
fn cargo_args(args: &[String]) -> Result<(Vec<String>, bool), String> {
    let mut packages = Vec::new();
    let (mut everything, mut release, mut rest) = (false, false, Vec::new());
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--release" => release = true,
            "--" => {
                rest.extend(iter.by_ref().cloned());
                break;
            }
            "all" => everything = true,
            flag if flag.starts_with('-') => {
                return Err(format!("unexpected option {flag:?} (cargo's go after --)"));
            }
            name => {
                let (_, package) = TARGETS
                    .iter()
                    .find(|(target, _)| *target == name)
                    .ok_or_else(|| {
                        format!("unknown target {name:?} (cli app gen previs md verse all)")
                    })?;
                packages.extend(["-p".to_string(), package.to_string()]);
            }
        }
    }
    if packages.is_empty() && !everything {
        return Err(format!("usage: cargo xtask build <target…>\n{USAGE}"));
    }
    let mut cargo = vec!["build".to_string()];
    if everything {
        cargo.push("--workspace".into());
    } else {
        cargo.extend(packages);
    }
    if release {
        cargo.push("--release".into());
    }
    cargo.extend(rest);
    Ok((cargo, release))
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let (cargo_args, release) = cargo_args(args)?;
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    println!("──────── cargo {} ────────", cargo_args.join(" "));
    let status = Command::new(cargo)
        .args(&cargo_args)
        .current_dir(root)
        .status()
        .map_err(|e| format!("cargo would not start: {e}"))?;
    if !status.success() {
        return Err("the build FAILED".into());
    }
    println!(
        "\nBuilt into target/{}/",
        if release { "release" } else { "debug" }
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn targets_by_name_become_packages() {
        let (cargo, release) = cargo_args(&args(&["app", "gen", "--release"])).unwrap();
        assert_eq!(
            cargo,
            [
                "build",
                "-p",
                "localgpt-app",
                "-p",
                "localgpt-gen",
                "--release"
            ]
        );
        assert!(release);
        let (cargo, _) = cargo_args(&args(&["all", "--", "--locked"])).unwrap();
        assert_eq!(cargo, ["build", "--workspace", "--locked"]);
        assert!(cargo_args(&args(&["website"])).is_err());
        assert!(cargo_args(&args(&[])).is_err());
    }
}
