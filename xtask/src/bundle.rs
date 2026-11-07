//! `cargo xtask bundle [--debug] [--dmg]`: the one LocalGPT desktop app
//! (`localgpt-app`) as `LocalGPT.app`, and a `.dmg` when asked (it was
//! `apps/app-desktop/macos/build-app.sh`). Output goes to
//! `apps/app-desktop/dist/`.
//!
//! Signing is ad hoc by default: the app runs on this Mac and nowhere else
//! without a Gatekeeper override. For a download other people can open,
//! `APPLE_SIGNING_IDENTITY="Developer ID Application: <name> (<team id>)"`
//! signs the app and the `.dmg` with the hardened runtime and a secure
//! timestamp, what notarization requires; `APPLE_NOTARY_PROFILE=<profile>`,
//! a keychain profile made once with `xcrun notarytool store-credentials`,
//! implies `--dmg`, submits it, waits, and staples the ticket to it.
//! `APP_FEATURES` names cargo features for the build (comma-separated).
//!
//! The app runs no model: it holds no model's configuration, and the agents
//! in its terminals bring their own. The 522 MB asset pack is fetched once
//! per machine (`cargo xtask fetch-assets`); the four starter tracks a
//! first launch opens are carried inside.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const USAGE: &str = "usage: cargo xtask bundle [--debug] [--dmg]

  --debug   bundle the debug build (quick, for trying the bundle here)
  --dmg     also make apps/app-desktop/dist/LocalGPT-<version>.dmg

  APPLE_SIGNING_IDENTITY   sign with a Developer ID (else ad hoc, this Mac only)
  APPLE_NOTARY_PROFILE     notarize the .dmg with this keychain profile (implies --dmg)
  APP_FEATURES             cargo features for the build, comma-separated
  LOCALGPT_STARTER_MUSIC   an asset root whose music/ holds the starter tracks";

struct Options {
    release: bool,
    dmg: bool,
    identity: Option<String>,
    notary: Option<String>,
    features: Option<String>,
}

fn options(args: &[String]) -> Result<Option<Options>, String> {
    let set = |key: &str| std::env::var(key).ok().filter(|v| !v.is_empty());
    let mut options = Options {
        release: true,
        dmg: false,
        identity: set("APPLE_SIGNING_IDENTITY"),
        notary: set("APPLE_NOTARY_PROFILE"),
        features: set("APP_FEATURES"),
    };
    for arg in args {
        match arg.as_str() {
            "--debug" => options.release = false,
            "--dmg" => options.dmg = true,
            "-h" | "--help" => return Ok(None),
            other => return Err(format!("unknown option {other:?}\n\n{USAGE}")),
        }
    }
    if options.notary.is_some() {
        if options.identity.is_none() {
            return Err("APPLE_NOTARY_PROFILE needs APPLE_SIGNING_IDENTITY".into());
        }
        options.dmg = true;
    }
    Ok(Some(options))
}

pub fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let Some(options) = options(args)? else {
        println!("{USAGE}");
        return Ok(());
    };
    if !cfg!(target_os = "macos") {
        return Err("LocalGPT.app is macOS's: this builds it on a Mac".into());
    }
    let version = version(root)?;
    let out = root.join("apps/app-desktop/dist");
    let app = out.join("LocalGPT.app");
    let work = Work::new()?;

    // The page a paired phone gets, which the release binary carries inside
    // it, built first so the embed finds it. A debug build reads it from
    // disk, so it is built for one too.
    println!("==> the phone's page (crates/app/ui)");
    let page = root.join("crates/app/ui");
    let npm = "npm";
    run_in(&page, npm, &["ci", "--no-audit", "--no-fund"])
        .map_err(|e| format!("{e} (the phone's page needs node 22)"))?;
    run_in(&page, npm, &["run", "build"])?;

    let profile = if options.release { "release" } else { "debug" };
    match &options.features {
        Some(features) => println!("==> cargo build ({profile}, features: {features})"),
        None => println!("==> cargo build ({profile})"),
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut build = vec!["build".to_string(), "-p".into(), "localgpt-app".into()];
    if options.release {
        build.push("--release".into());
    }
    if let Some(features) = &options.features {
        build.push(format!("--features={features}"));
    }
    run_in(
        root,
        &cargo,
        &build.iter().map(String::as_str).collect::<Vec<_>>(),
    )?;
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"));
    let binary = target.join(profile).join("localgpt-app");

    println!("==> assembling {}", app.display());
    let _ = std::fs::remove_dir_all(&app);
    let (macos, resources) = (app.join("Contents/MacOS"), app.join("Contents/Resources"));
    for dir in [&macos, &resources] {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::copy(&binary, macos.join("localgpt-app"))
        .map_err(|e| format!("{}: {e}", binary.display()))?;

    let icon = icon(root, &work.0, &resources);
    if !icon {
        println!("    warn: couldn't render the app icon; bundling without one");
    }
    std::fs::write(app.join("Contents/Info.plist"), info_plist(&version, icon))
        .map_err(|e| format!("Info.plist: {e}"))?;
    starter_music(root, &resources)?;

    match &options.identity {
        Some(identity) => {
            println!("==> signing with {identity} (hardened runtime)");
            run_in(
                root,
                "codesign",
                &[
                    "--force",
                    "--options",
                    "runtime",
                    "--timestamp",
                    "--sign",
                    identity,
                    path(&app),
                ],
            )?;
        }
        None => {
            println!("==> ad-hoc signing (this Mac only; see APPLE_SIGNING_IDENTITY)");
            run_in(root, "codesign", &["--force", "--sign", "-", path(&app)])?;
        }
    }
    run_in(root, "codesign", &["--verify", "--strict", path(&app)])?;

    if options.dmg {
        let dmg = out.join(format!("LocalGPT-{version}.dmg"));
        println!("==> {}", dmg.display());
        let stage = work.0.join("dmg");
        std::fs::create_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
        run_in(
            root,
            "ditto",
            &[path(&app), path(&stage.join("LocalGPT.app"))],
        )?;
        // A Mac's alone (the command stops on any other system above); the
        // gate keeps the xtask building everywhere.
        #[cfg(unix)]
        std::os::unix::fs::symlink("/Applications", stage.join("Applications"))
            .map_err(|e| format!("the Applications link: {e}"))?;
        let _ = std::fs::remove_file(&dmg);
        run_in(
            root,
            "hdiutil",
            &[
                "create",
                "-volname",
                "LocalGPT",
                "-srcfolder",
                path(&stage),
                "-ov",
                "-format",
                "UDZO",
                path(&dmg),
            ],
        )?;
        if let Some(identity) = &options.identity {
            run_in(
                root,
                "codesign",
                &["--force", "--timestamp", "--sign", identity, path(&dmg)],
            )?;
        }
        if let Some(profile) = &options.notary {
            println!("==> notarizing (this waits for Apple)");
            run_in(
                root,
                "xcrun",
                &[
                    "notarytool",
                    "submit",
                    path(&dmg),
                    "--keychain-profile",
                    profile,
                    "--wait",
                ],
            )?;
            run_in(root, "xcrun", &["stapler", "staple", path(&dmg)])?;
            run_in(
                root,
                "spctl",
                &[
                    "--assess",
                    "--type",
                    "open",
                    "--context",
                    "context:primary-signature",
                    "--verbose",
                    path(&dmg),
                ],
            )?;
        }
    }

    let _ = Command::new("du").args(["-sh", path(&app)]).status();
    println!("done: open \"{}\"", app.display());
    Ok(())
}

/// The workspace's one version (`[workspace.package]`).
fn version(root: &Path) -> Result<String, String> {
    let manifest =
        std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("Cargo.toml: {e}"))?;
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.split('"').next())
        .map(str::to_string)
        .ok_or_else(|| "Cargo.toml names no version".into())
}

/// The family mark as the app's icon: QuickLook renders the SVG, sips makes
/// the sizes, iconutil packs them. Best effort: the app works without one.
fn icon(root: &Path, work: &Path, resources: &Path) -> bool {
    let svg = root.join("website/static/logo/localgpt-icon.svg");
    let quiet = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .output()
            .is_ok_and(|out| out.status.success())
    };
    if !quiet(
        "qlmanage",
        &["-t", "-s", "1024", "-o", path(work), path(&svg)],
    ) {
        return false;
    }
    let master = work.join(format!(
        "{}.png",
        svg.file_name().unwrap_or_default().to_string_lossy()
    ));
    let set = work.join("AppIcon.iconset");
    if !master.is_file() || std::fs::create_dir_all(&set).is_err() {
        return false;
    }
    for size in [16u32, 32, 128, 256, 512] {
        for (scale, suffix) in [(1, ""), (2, "@2x")] {
            let side = (size * scale).to_string();
            let file = set.join(format!("icon_{size}x{size}{suffix}.png"));
            if !quiet(
                "sips",
                &["-z", &side, &side, path(&master), "--out", path(&file)],
            ) {
                return false;
            }
        }
    }
    quiet(
        "iconutil",
        &[
            "-c",
            "icns",
            path(&set),
            "-o",
            path(&resources.join("AppIcon.icns")),
        ],
    )
}

fn info_plist(version: &str, icon: bool) -> String {
    let icon = if icon {
        "<key>CFBundleIconFile</key><string>AppIcon</string>"
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>LocalGPT</string>
  <key>CFBundleDisplayName</key><string>LocalGPT</string>
  <key>CFBundleIdentifier</key><string>app.localgpt.desktop</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleExecutable</key><string>localgpt-app</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  {icon}
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSLocalNetworkUsageDescription</key>
  <string>LocalGPT uses your local network to host and join collaborative world-building sessions.</string>
  <key>NSBonjourServices</key>
  <array><string>_localgpt-world._udp</string></array>
</dict>
</plist>
"#
    )
}

/// The four CC0 starter tracks, which a cold start opens
/// (`crates/app/src/starter.rs` looks in `Resources/starter`): a few
/// megabytes, where the pack is fetched once per machine instead. Not in
/// `Resources/assets`: Verse takes that folder, whenever it exists, as its
/// one asset root, and would then miss the pack's models. From the asset
/// root `LOCALGPT_STARTER_MUSIC` names, an assets checkout, or the shared
/// pack, whichever holds the tracks themselves first (a checkout without
/// LFS holds pointers). Without them the bundle works, but its first launch
/// opens nothing.
fn starter_music(root: &Path, resources: &Path) -> Result<(), String> {
    let mut roots: Vec<PathBuf> = std::env::var_os("LOCALGPT_STARTER_MUSIC")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    roots.extend(crate::assets::checkouts(root));
    roots.extend(crate::assets::pack_root().ok());
    let mut found = None;
    for music in roots.iter().map(|root| root.join("music")) {
        if !music.join("music.json").is_file() {
            continue;
        }
        let tracks = tracks(&music);
        if tracks.iter().any(|track| lfs_pointer(track)) {
            println!(
                "    warn: {} holds LFS pointers, not tracks (git lfs pull there)",
                music.display()
            );
            continue;
        }
        found = Some((music, tracks));
        break;
    }
    let Some((music, tracks)) = found else {
        println!(
            "    warn: no starter music (LOCALGPT_STARTER_MUSIC, ../localgpt-world-assets or the pack); a first launch opens nothing"
        );
        return Ok(());
    };
    let into = resources.join("starter/music");
    std::fs::create_dir_all(&into).map_err(|e| format!("{}: {e}", into.display()))?;
    let notice = music.join("NOTICE");
    let files = tracks
        .into_iter()
        .chain([music.join("music.json")])
        .chain(notice.is_file().then_some(notice));
    for file in files {
        let name = file.file_name().unwrap_or_default();
        std::fs::copy(&file, into.join(name)).map_err(|e| format!("{}: {e}", file.display()))?;
    }
    println!("    starter music from {}", music.display());
    Ok(())
}

fn tracks(music: &Path) -> Vec<PathBuf> {
    let mut tracks: Vec<PathBuf> = std::fs::read_dir(music)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|ext| ext == "mp3"))
                .collect()
        })
        .unwrap_or_default();
    tracks.sort();
    tracks
}

/// A Git LFS pointer where a track should be: a checkout without LFS.
fn lfs_pointer(file: &Path) -> bool {
    let mut head = [0u8; 24];
    std::fs::File::open(file)
        .and_then(|mut f| std::io::Read::read(&mut f, &mut head))
        .is_ok_and(|n| head[..n].starts_with(b"version https://git-lfs"))
}

/// A path as an argument; the bundle's paths are this checkout's, UTF-8.
fn path(path: &Path) -> &str {
    path.to_str().unwrap_or_default()
}

fn run_in(dir: &Path, program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program)
        .args(args)
        .current_dir(dir)
        .status()
        .map_err(|e| format!("{program} would not start: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} {} failed ({status})", args.join(" ")))
    }
}

/// A scratch folder for the icon and the `.dmg`'s stage, gone at the end.
struct Work(PathBuf);

impl Work {
    fn new() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!("localgpt-bundle-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self(dir))
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_the_workspaces() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let version = version(root).unwrap();
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
        assert!(info_plist(&version, true).contains(&format!(
            "<key>CFBundleShortVersionString</key><string>{version}</string>"
        )));
        assert!(!info_plist(&version, false).contains("CFBundleIconFile"));
    }

    #[test]
    fn an_lfs_pointer_is_told_from_a_track() {
        let dir = std::env::temp_dir().join(format!("xtask-lfs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (pointer, track) = (dir.join("a.mp3"), dir.join("b.mp3"));
        std::fs::write(
            &pointer,
            "version https://git-lfs.github.com/spec/v1\noid sha256:0\n",
        )
        .unwrap();
        std::fs::write(&track, b"ID3\x03\x00junk").unwrap();
        assert!(lfs_pointer(&pointer));
        assert!(!lfs_pointer(&track));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
