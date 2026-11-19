//! Builds the player, bundles it with a world, signs it and starts it.
//!
//! ```text
//! cargo run -- sim [--console]      # the iPad simulator nearest a 12.9-inch iPad Pro
//! cargo run -- device [--console]   # a paired iPad, signed for development
//! ```
//!
//! `--world <name>` picks a package under `website-world/worlds` (default
//! `verse-amber-drift`); its files come from the `localgpt-world-assets`
//! checkout beside this one, or `$LOCALGPT_WORLD_ASSETS`. `--udid` picks the
//! simulator or the device, `--env KEY=VALUE` passes a variable to the app,
//! `--no-launch` stops after installing.
//!
//! On a device the bundle is signed with an Apple Development certificate of
//! the team that holds the `app.localgpt` App ID (or `--team`), never
//! another team's, and a development profile of that team listing the iPad.
//! `--provision` has Xcode make them when they are missing: it registers the
//! iPad with the team and creates a certificate when none is valid.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::{env, fs};

const BUNDLE_ID: &str = "app.localgpt.spike.player";
const EXECUTABLE: &str = "ipad-player";
const APP: &str = "Player.app";
const MIN_OS: &str = "17.0";
/// The SDK the binary says it was linked against (see `assemble`).
const LEGACY_SDK: &str = "26.0";
const DEFAULT_WORLD: &str = "verse-amber-drift";
const USAGE: &str = "usage: cargo run -- sim|device [--world <name>] [--udid <id>] [--team <id>] \
                     [--env KEY=VALUE] [--console] [--no-launch] [--provision]";

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Sim,
    Device,
}

impl Target {
    fn triple(self) -> &'static str {
        match self {
            Target::Sim => "aarch64-apple-ios-sim",
            Target::Device => "aarch64-apple-ios",
        }
    }
}

struct Args {
    target: Target,
    world: String,
    udid: Option<String>,
    team: Option<String>,
    env: Vec<(String, String)>,
    console: bool,
    launch: bool,
    provision: bool,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("bundle: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse(env::args().skip(1))?;
    let spike = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let repo = spike.join("../..");
    build(&spike, args.target)?;
    let app = assemble(&spike, &repo, &args)?;
    match args.target {
        Target::Sim => simulator(&app, &args),
        Target::Device => device(&spike, &app, &args),
    }
}

fn parse(mut words: impl Iterator<Item = String>) -> Result<Args, String> {
    let target = match words.next().as_deref() {
        Some("sim") => Target::Sim,
        Some("device") => Target::Device,
        _ => return Err(USAGE.into()),
    };
    let mut args = Args {
        target,
        world: DEFAULT_WORLD.into(),
        udid: None,
        team: None,
        env: Vec::new(),
        console: false,
        launch: true,
        provision: false,
    };
    while let Some(flag) = words.next() {
        match flag.as_str() {
            "--world" => args.world = words.next().ok_or("--world takes a name")?,
            "--udid" => args.udid = Some(words.next().ok_or("--udid takes an id")?),
            "--team" => args.team = Some(words.next().ok_or("--team takes a team id")?),
            "--env" => {
                let pair = words.next().ok_or("--env takes KEY=VALUE")?;
                let (key, value) = pair.split_once('=').ok_or("--env takes KEY=VALUE")?;
                args.env.push((key.into(), value.into()));
            }
            "--console" => args.console = true,
            "--no-launch" => args.launch = false,
            "--provision" => args.provision = true,
            other => return Err(format!("unknown option {other}\n{USAGE}")),
        }
    }
    Ok(args)
}

fn build(spike: &Path, target: Target) -> Result<(), String> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    status(
        Command::new(cargo)
            .current_dir(spike)
            .args(["build", "--release", "--target", target.triple()])
            .args(["--bin", EXECUTABLE])
            .env("IPHONEOS_DEPLOYMENT_TARGET", MIN_OS),
        "cargo build",
    )
}

/// `Player.app`: the executable, its Info.plist, and the package under
/// `assets/world`, where the player and Bevy's asset server look.
fn assemble(spike: &Path, repo: &Path, args: &Args) -> Result<PathBuf, String> {
    let out = spike
        .join("target")
        .join(args.target.triple())
        .join("release");
    let app = out.join(APP);
    if app.exists() {
        fs::remove_dir_all(&app).map_err(io(&app))?;
    }
    let world = app.join("assets/world");
    let files = world.join("assets");
    fs::create_dir_all(&files).map_err(io(&files))?;
    // winit 0.30, Bevy 0.20's, has no UIScene life cycle, and iOS 27 will
    // not launch an app linked against its SDK without one ("UIScene life
    // cycle is required for apps built with this SDK"). Marked as linked
    // against the iOS 26 SDK, the binary takes the app-delegate path iOS 27
    // still allows. A shipping app needs a winit with scenes.
    let platform = match args.target {
        Target::Sim => "iossim",
        Target::Device => "ios",
    };
    status(
        Command::new("xcrun")
            .args(["vtool", "-set-build-version", platform, MIN_OS, LEGACY_SDK])
            .args(["-replace", "-output"])
            .arg(app.join(EXECUTABLE))
            .arg(out.join(EXECUTABLE)),
        "vtool",
    )?;
    fs::write(app.join("Info.plist"), info_plist(args.target)).map_err(io(&app))?;

    let package = repo
        .join("website-world/worlds")
        .join(format!("{}.world", args.world));
    copy(&package.join("manifest.json"), &world.join("manifest.json"))?;
    let source = world_files(repo, &package, &args.world)?;
    let (mut count, mut bytes) = (0, 0);
    for entry in fs::read_dir(&source).map_err(io(&source))? {
        let entry = entry.map_err(io(&source))?;
        if entry.path().is_file() {
            bytes += copy(&entry.path(), &files.join(entry.file_name()))?;
            count += 1;
        }
    }
    eprintln!(
        "bundle: {} holds {} ({count} files, {:.1} MB)",
        app.display(),
        args.world,
        bytes as f64 / 1e6
    );
    Ok(app)
}

/// The package's files: the asset checkout's, else the site's assembled copy.
fn world_files(repo: &Path, package: &Path, world: &str) -> Result<PathBuf, String> {
    let checkout = env::var_os("LOCALGPT_WORLD_ASSETS")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("../localgpt-world-assets"));
    [
        checkout
            .join("web/worlds")
            .join(format!("{world}.world/assets")),
        package.join("assets"),
    ]
    .into_iter()
    .find(|path| path.is_dir())
    .ok_or_else(|| {
        format!(
            "no files for {world}: clone localgpt-world-assets beside this checkout, \
             or set LOCALGPT_WORLD_ASSETS"
        )
    })
}

/// iPad only, every orientation and resizable (Split View and Stage Manager
/// resize it, which is a test too), audio in the background, no status bar,
/// a pointer from a trackpad, and `Documents/` in the Files app.
/// `UILaunchScreen` is what gets a bundle without a storyboard the screen's
/// native size instead of a letterboxed one.
fn info_plist(target: Target) -> String {
    let (platform, sdk) = match target {
        Target::Sim => ("iPhoneSimulator", "iphonesimulator"),
        Target::Device => ("iPhoneOS", "iphoneos"),
    };
    let orientations = "<array>\
        <string>UIInterfaceOrientationPortrait</string>\
        <string>UIInterfaceOrientationPortraitUpsideDown</string>\
        <string>UIInterfaceOrientationLandscapeLeft</string>\
        <string>UIInterfaceOrientationLandscapeRight</string>\
        </array>";
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleDevelopmentRegion</key><string>en</string>
	<key>CFBundleDisplayName</key><string>Player</string>
	<key>CFBundleExecutable</key><string>{EXECUTABLE}</string>
	<key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
	<key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
	<key>CFBundleName</key><string>Player</string>
	<key>CFBundlePackageType</key><string>APPL</string>
	<key>CFBundleShortVersionString</key><string>0.0.1</string>
	<key>CFBundleVersion</key><string>1</string>
	<key>CFBundleSupportedPlatforms</key><array><string>{platform}</string></array>
	<key>DTPlatformName</key><string>{sdk}</string>
	<key>LSRequiresIPhoneOS</key><true/>
	<key>MinimumOSVersion</key><string>{MIN_OS}</string>
	<key>UIDeviceFamily</key><array><integer>2</integer></array>
	<key>UILaunchScreen</key><dict/>
	<key>UIBackgroundModes</key><array><string>audio</string></array>
	<key>UIRequiresFullScreen</key><false/>
	<key>UISupportedInterfaceOrientations</key>{orientations}
	<key>UISupportedInterfaceOrientations~ipad</key>{orientations}
	<key>UIApplicationSupportsIndirectInputEvents</key><true/>
	<key>UIStatusBarHidden</key><true/>
	<key>UIFileSharingEnabled</key><true/>
	<key>LSSupportsOpeningDocumentsInPlace</key><true/>
</dict>
</plist>
"#
    )
}

fn simulator(app: &Path, args: &Args) -> Result<(), String> {
    let udid = match &args.udid {
        Some(udid) => udid.clone(),
        None => pick_simulator()?,
    };
    // Booting one that is already up fails, which is fine.
    let _ = Command::new("xcrun")
        .args(["simctl", "boot", &udid])
        .stderr(Stdio::null())
        .status();
    status(
        Command::new("xcrun")
            .args(["simctl", "bootstatus", &udid, "-b"])
            .stdout(Stdio::null()),
        "simctl bootstatus",
    )?;
    status(
        Command::new("codesign")
            .args(["--force", "--sign", "-", "--timestamp=none"])
            .arg(app),
        "codesign",
    )?;
    status(
        Command::new("xcrun")
            .args(["simctl", "install", &udid])
            .arg(app),
        "simctl install",
    )?;
    if !args.launch {
        return Ok(());
    }
    // `--stdout=`/`--stderr=` files fail here (the launch fails outright
    // for a path on another volume), so the output is the console's or none.
    let mut launch = Command::new("xcrun");
    launch.args(["simctl", "launch", "--terminate-running-process"]);
    if args.console {
        launch.arg("--console");
    }
    launch.args([udid.as_str(), BUNDLE_ID]);
    for (key, value) in &args.env {
        launch.env(format!("SIMCTL_CHILD_{key}"), value);
    }
    status(&mut launch, "simctl launch")?;
    if !args.console {
        eprintln!("bundle: running on simulator {udid}; --console shows its output");
    }
    Ok(())
}

/// The available simulator closest to a 12.9-inch iPad Pro, on the newest
/// runtime that has it.
fn pick_simulator() -> Result<String, String> {
    let listing = output(
        Command::new("xcrun").args(["simctl", "list", "devices", "available", "-j"]),
        "simctl list",
    )?;
    let listing: serde_json::Value =
        serde_json::from_str(&listing).map_err(|e| format!("simctl list: {e}"))?;
    let mut best: Option<(u8, String, String, String)> = None;
    for (runtime, devices) in listing["devices"].as_object().into_iter().flatten() {
        for device in devices.as_array().into_iter().flatten() {
            let name = device["name"].as_str().unwrap_or_default();
            let rank = if name.starts_with("iPad Pro (12.9-inch)") {
                3
            } else if name.starts_with("iPad Pro 13-inch") {
                2
            } else if name.starts_with("iPad") {
                1
            } else {
                continue;
            };
            let udid = device["udid"].as_str().unwrap_or_default().to_string();
            let candidate = (rank, runtime.clone(), udid, name.to_string());
            if best
                .as_ref()
                .is_none_or(|b| (b.0, &b.1) < (candidate.0, &candidate.1))
            {
                best = Some(candidate);
            }
        }
    }
    let (_, runtime, udid, name) = best.ok_or("no iPad simulator is installed")?;
    eprintln!("bundle: simulator {name} ({runtime})");
    Ok(udid)
}

fn device(spike: &Path, app: &Path, args: &Args) -> Result<(), String> {
    let udid = match &args.udid {
        Some(udid) => udid.clone(),
        None => pick_device(spike)?,
    };
    let team = match &args.team {
        Some(team) => team.clone(),
        None => localgpt_team(&profiles(spike)?)?,
    };
    let mut found = signing(spike, &team, &udid)?;
    if found.is_none() && args.provision {
        provision(spike, &team, &udid)?;
        found = signing(spike, &team, &udid)?;
    }
    let (identity, profile) = found.ok_or_else(|| {
        format!(
            "no Apple Development certificate and profile of team {team} sign \
             {BUNDLE_ID} for {udid}. Run again with --provision to have Xcode make them \
             (it registers the iPad with the team and creates a certificate if none is \
             valid), or in Xcode: Settings > Accounts > the team > Manage Certificates > \
             + > Apple Development, then build any app with this bundle id for the iPad."
        )
    })?;

    copy(&profile.path, &app.join("embedded.mobileprovision"))?;
    let entitlements = app.with_file_name("player.entitlements");
    fs::write(
        &entitlements,
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>application-identifier</key><string>{team}.{BUNDLE_ID}</string>
	<key>com.apple.developer.team-identifier</key><string>{team}</string>
	<key>get-task-allow</key><true/>
</dict>
</plist>
"#
        ),
    )
    .map_err(io(&entitlements))?;
    status(
        Command::new("codesign")
            .args(["--force", "--sign", &identity.sha1, "--entitlements"])
            .arg(&entitlements)
            .args(["--generate-entitlement-der", "--timestamp=none"])
            .arg(app),
        "codesign",
    )?;
    eprintln!(
        "bundle: signed by {} with the profile {}",
        identity.name, profile.name
    );
    status(
        Command::new("xcrun")
            .args(["devicectl", "device", "install", "app", "--device", &udid])
            .arg(app),
        "devicectl install",
    )?;
    if !args.launch {
        return Ok(());
    }
    let mut launch = Command::new("xcrun");
    launch.args([
        "devicectl",
        "device",
        "process",
        "launch",
        "--device",
        &udid,
    ]);
    launch.arg("--terminate-existing");
    if args.console {
        launch.arg("--console");
    }
    launch.arg(BUNDLE_ID);
    for (key, value) in &args.env {
        launch.env(format!("DEVICECTL_CHILD_{key}"), value);
    }
    status(&mut launch, "devicectl launch")
}

/// The first paired physical iPad.
fn pick_device(spike: &Path) -> Result<String, String> {
    let json = spike.join("target/devices.json");
    fs::create_dir_all(spike.join("target")).map_err(io(spike))?;
    status(
        Command::new("xcrun")
            .args(["devicectl", "list", "devices", "--quiet", "--json-output"])
            .arg(&json),
        "devicectl list",
    )?;
    let text = fs::read_to_string(&json).map_err(io(&json))?;
    let listing: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("devicectl list: {e}"))?;
    for device in listing["result"]["devices"]
        .as_array()
        .into_iter()
        .flatten()
    {
        let hardware = &device["hardwareProperties"];
        if hardware["reality"] == "physical"
            && hardware["deviceType"] == "iPad"
            && device["connectionProperties"]["pairingState"] == "paired"
            && let Some(udid) = hardware["udid"].as_str()
        {
            eprintln!(
                "bundle: device {}",
                device["deviceProperties"]["name"].as_str().unwrap_or(udid)
            );
            return Ok(udid.to_string());
        }
    }
    Err("no paired iPad: connect it, unlock it and trust this Mac".into())
}

/// A code-signing identity: its hash, its name and its certificate (DER,
/// base64, as a profile lists it).
#[derive(Clone)]
struct Identity {
    sha1: String,
    name: String,
    cert: String,
}

/// What signing needs from a provisioning profile.
#[derive(Clone)]
struct Profile {
    path: PathBuf,
    name: String,
    team: String,
    app_id: String,
    expires: String,
    development: bool,
    devices: String,
    certs: Vec<String>,
}

/// A valid Apple Development identity of `team`, and the development
/// profile of that team that lists the device and this app, latest first.
fn signing(spike: &Path, team: &str, udid: &str) -> Result<Option<(Identity, Profile)>, String> {
    let identities = identities(team)?;
    let now = output(
        Command::new("date").args(["-u", "+%Y-%m-%dT%H:%M:%SZ"]),
        "date",
    )?;
    let app_id = format!("{team}.{BUNDLE_ID}");
    let mut best: Option<(Identity, Profile)> = None;
    for profile in profiles(spike)? {
        let covers = profile.app_id == app_id
            || profile
                .app_id
                .strip_suffix('*')
                .is_some_and(|prefix| app_id.starts_with(prefix));
        if profile.team != team
            || !profile.development
            || profile.expires.as_str() <= now.as_str()
            || !covers
            || !profile
                .devices
                .contains(&format!("<string>{udid}</string>"))
        {
            continue;
        }
        let Some(identity) = identities.iter().find(|i| profile.certs.contains(&i.cert)) else {
            continue;
        };
        if best
            .as_ref()
            .is_none_or(|(_, b)| b.expires < profile.expires)
        {
            best = Some((identity.clone(), profile));
        }
    }
    Ok(best)
}

/// The valid Apple Development identities whose certificate is `team`'s.
fn identities(team: &str) -> Result<Vec<Identity>, String> {
    let listing = output(
        Command::new("security").args(["find-identity", "-v", "-p", "codesigning"]),
        "security find-identity",
    )?;
    let mut found = Vec::new();
    for line in listing.lines() {
        let Some((_, rest)) = line.trim().split_once(") ") else {
            continue;
        };
        let Some((sha1, name)) = rest.split_once(' ') else {
            continue;
        };
        let name = name.trim_matches('"');
        if !name.starts_with("Apple Development:") {
            continue;
        }
        let Some(cert) = certificate(name, sha1)? else {
            continue;
        };
        if organizational_unit(&cert)?.as_deref() == Some(team) {
            found.push(Identity {
                sha1: sha1.to_string(),
                name: name.to_string(),
                cert,
            });
        }
    }
    Ok(found)
}

/// The certificate named `name` whose SHA-1 is `sha1`, as base64 DER: one
/// name can carry several, expired ones and other teams' among them.
fn certificate(name: &str, sha1: &str) -> Result<Option<String>, String> {
    let listing = output(
        Command::new("security").args(["find-certificate", "-a", "-Z", "-p", "-c", name]),
        "security find-certificate",
    )?;
    let (mut hash, mut pem, mut inside) = (String::new(), String::new(), false);
    for line in listing.lines() {
        if let Some(value) = line.strip_prefix("SHA-1 hash: ") {
            hash = value.trim().to_string();
        } else if line.starts_with("-----BEGIN CERTIFICATE-----") {
            (inside, pem) = (true, String::new());
        } else if line.starts_with("-----END CERTIFICATE-----") {
            inside = false;
            if hash.eq_ignore_ascii_case(sha1) {
                return Ok(Some(pem));
            }
        } else if inside {
            pem.push_str(line.trim());
        }
    }
    Ok(None)
}

/// The team a certificate belongs to: its subject's OU.
fn organizational_unit(cert: &str) -> Result<Option<String>, String> {
    let wrapped: Vec<String> = cert
        .as_bytes()
        .chunks(64)
        .map(|chunk| String::from_utf8_lossy(chunk).into_owned())
        .collect();
    let pem = format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
        wrapped.join("\n")
    );
    let mut child = Command::new("openssl")
        .args(["x509", "-noout", "-subject"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("openssl: {e}"))?;
    child
        .stdin
        .take()
        .ok_or("openssl: no stdin")?
        .write_all(pem.as_bytes())
        .map_err(|e| format!("openssl: {e}"))?;
    let out = child
        .wait_with_output()
        .map_err(|e| format!("openssl: {e}"))?;
    let subject = String::from_utf8_lossy(&out.stdout);
    Ok(subject
        .split([',', '/'])
        .find_map(|part| {
            part.trim()
                .strip_prefix("OU=")
                .or(part.trim().strip_prefix("OU = "))
        })
        .map(|ou| ou.trim().to_string()))
}

/// Every provisioning profile Xcode keeps, read through `security` and
/// `plutil`.
fn profiles(spike: &Path) -> Result<Vec<Profile>, String> {
    let home = PathBuf::from(env::var_os("HOME").ok_or("no HOME")?);
    let scratch = spike.join("target/profile.plist");
    fs::create_dir_all(spike.join("target")).map_err(io(spike))?;
    let mut found = Vec::new();
    for dir in [
        "Library/Developer/Xcode/UserData/Provisioning Profiles",
        "Library/MobileDevice/Provisioning Profiles",
    ] {
        let Ok(entries) = fs::read_dir(home.join(dir)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "mobileprovision") {
                continue;
            }
            let Ok(decoded) = Command::new("security")
                .args(["cms", "-D", "-i"])
                .arg(&path)
                .stderr(Stdio::null())
                .output()
            else {
                continue;
            };
            if !decoded.status.success() {
                continue;
            }
            fs::write(&scratch, &decoded.stdout).map_err(io(&scratch))?;
            let key = |key: &str| {
                Command::new("plutil")
                    .args(["-extract", key, "raw", "-o", "-"])
                    .arg(&scratch)
                    .stderr(Stdio::null())
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                    .unwrap_or_default()
            };
            let count: usize = key("DeveloperCertificates").parse().unwrap_or(0);
            found.push(Profile {
                name: key("Name"),
                team: key("TeamIdentifier.0"),
                app_id: key("Entitlements.application-identifier"),
                expires: key("ExpirationDate"),
                development: key("Entitlements.get-task-allow") == "true",
                devices: String::from_utf8_lossy(&decoded.stdout).into_owned(),
                certs: (0..count)
                    .map(|i| key(&format!("DeveloperCertificates.{i}")))
                    .collect(),
                path,
            });
        }
    }
    Ok(found)
}

/// The team holding the `app.localgpt` App ID: the one that publishes
/// LocalGPT, and so the one to sign its spikes.
fn localgpt_team(profiles: &[Profile]) -> Result<String, String> {
    let mut teams: Vec<&str> = profiles
        .iter()
        .filter(|p| {
            p.app_id
                .strip_prefix(&format!("{}.", p.team))
                .is_some_and(|id| id == "app.localgpt" || id.starts_with("app.localgpt."))
        })
        .map(|p| p.team.as_str())
        .collect();
    teams.sort_unstable();
    teams.dedup();
    match teams.as_slice() {
        [team] => Ok(team.to_string()),
        [] => Err(
            "no profile holds the app.localgpt App ID: pass --team with the team \
                   that publishes LocalGPT"
                .into(),
        ),
        _ => Err(format!(
            "several teams hold app.localgpt ({}): pass --team",
            teams.join(", ")
        )),
    }
}

/// Has Xcode make what a development install needs, through a throwaway
/// project with nothing in it: automatic signing for the team, built for
/// the device, which registers the device, creates a certificate when none
/// is valid and writes the profile. The build itself may fail; the profile
/// is made before it would.
fn provision(spike: &Path, team: &str, udid: &str) -> Result<(), String> {
    let root = spike.join("target/provision");
    let project = root.join("Provision.xcodeproj");
    let schemes = project.join("xcshareddata/xcschemes");
    fs::create_dir_all(&schemes).map_err(io(&schemes))?;
    let id = |n: u32| format!("A1{n:022X}");
    let pbxproj = format!(
        r#"// !$*UTF8*$!
{{
	archiveVersion = 1;
	classes = {{
	}};
	objectVersion = 56;
	objects = {{
		{app} /* Provision.app */ = {{isa = PBXFileReference; explicitFileType = wrapper.application; includeInIndex = 0; path = Provision.app; sourceTree = BUILT_PRODUCTS_DIR; }};
		{main} = {{
			isa = PBXGroup;
			children = (
				{products},
			);
			sourceTree = "<group>";
		}};
		{products} /* Products */ = {{
			isa = PBXGroup;
			children = (
				{app},
			);
			name = Products;
			sourceTree = "<group>";
		}};
		{target} /* Provision */ = {{
			isa = PBXNativeTarget;
			buildConfigurationList = {target_list};
			buildPhases = (
			);
			buildRules = (
			);
			dependencies = (
			);
			name = Provision;
			productName = Provision;
			productReference = {app};
			productType = "com.apple.product-type.application";
		}};
		{project} /* Project object */ = {{
			isa = PBXProject;
			attributes = {{
				LastUpgradeCheck = 1600;
				TargetAttributes = {{
					{target} = {{
						DevelopmentTeam = {team};
						ProvisioningStyle = Automatic;
					}};
				}};
			}};
			buildConfigurationList = {project_list};
			compatibilityVersion = "Xcode 14.0";
			developmentRegion = en;
			hasScannedForEncodings = 0;
			knownRegions = (
				en,
				Base,
			);
			mainGroup = {main};
			productRefGroup = {products};
			projectDirPath = "";
			projectRoot = "";
			targets = (
				{target},
			);
		}};
		{project_debug} /* Debug */ = {{
			isa = XCBuildConfiguration;
			buildSettings = {{
				IPHONEOS_DEPLOYMENT_TARGET = {MIN_OS};
				SDKROOT = iphoneos;
			}};
			name = Debug;
		}};
		{target_debug} /* Debug */ = {{
			isa = XCBuildConfiguration;
			buildSettings = {{
				CODE_SIGN_STYLE = Automatic;
				DEVELOPMENT_TEAM = {team};
				GENERATE_INFOPLIST_FILE = YES;
				INFOPLIST_KEY_UILaunchScreen_Generation = YES;
				PRODUCT_BUNDLE_IDENTIFIER = {BUNDLE_ID};
				PRODUCT_NAME = Provision;
				TARGETED_DEVICE_FAMILY = 2;
			}};
			name = Debug;
		}};
		{project_list} = {{
			isa = XCConfigurationList;
			buildConfigurations = (
				{project_debug},
			);
			defaultConfigurationIsVisible = 0;
			defaultConfigurationName = Debug;
		}};
		{target_list} = {{
			isa = XCConfigurationList;
			buildConfigurations = (
				{target_debug},
			);
			defaultConfigurationIsVisible = 0;
			defaultConfigurationName = Debug;
		}};
	}};
	rootObject = {project};
}}
"#,
        app = id(1),
        main = id(2),
        products = id(3),
        target = id(4),
        target_list = id(5),
        project = id(6),
        project_list = id(7),
        project_debug = id(8),
        target_debug = id(9),
    );
    fs::write(project.join("project.pbxproj"), pbxproj).map_err(io(&project))?;
    let scheme = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Scheme LastUpgradeVersion = "1600" version = "1.7">
   <BuildAction parallelizeBuildables = "YES" buildImplicitDependencies = "YES">
      <BuildActionEntries>
         <BuildActionEntry buildForRunning = "YES" buildForTesting = "YES" buildForProfiling = "YES" buildForArchiving = "YES" buildForAnalyzing = "YES">
            <BuildableReference BuildableIdentifier = "primary" BlueprintIdentifier = "{target}" BuildableName = "Provision.app" BlueprintName = "Provision" ReferencedContainer = "container:Provision.xcodeproj">
            </BuildableReference>
         </BuildActionEntry>
      </BuildActionEntries>
   </BuildAction>
</Scheme>
"#,
        target = id(4),
    );
    fs::write(schemes.join("Provision.xcscheme"), scheme).map_err(io(&schemes))?;
    eprintln!("bundle: asking Xcode to provision {BUNDLE_ID} for team {team} and device {udid}");
    let _ = Command::new("xcodebuild")
        .arg("-project")
        .arg(&project)
        .args(["-scheme", "Provision", "-configuration", "Debug"])
        .args(["-destination", &format!("id={udid}")])
        .arg("-derivedDataPath")
        .arg(root.join("derived"))
        .args([
            "-allowProvisioningUpdates",
            "-allowProvisioningDeviceRegistration",
            "build",
        ])
        .status();
    Ok(())
}

fn status(command: &mut Command, what: &str) -> Result<(), String> {
    let status = command.status().map_err(|e| format!("{what}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({status})"))
    }
}

fn output(command: &mut Command, what: &str) -> Result<String, String> {
    let out = command
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("{what}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{what} failed ({})", out.status));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn copy(from: &Path, to: &Path) -> Result<u64, String> {
    fs::copy(from, to).map_err(|e| format!("{} -> {}: {e}", from.display(), to.display()))
}

fn io(path: &Path) -> impl Fn(std::io::Error) -> String + '_ {
    move |e| format!("{}: {e}", path.display())
}
