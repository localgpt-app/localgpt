//! The family's sites, each a Cloudflare Worker of static assets: deploying
//! them, assembling the two that are put together from the crates, and
//! adding a world to localgpt.world's gallery (they were
//! `bin/web/deploy.sh`, each site's `deploy.sh`, `website-md/scripts/
//! assemble.sh`, `website-world/scripts/assemble.mjs` and `add-world.mjs`).
//! Wrangler, Zola, wasm-pack and gltf-transform are tools the commands run,
//! as `bundle` runs codesign.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const DEPLOY_USAGE: &str = "  deploy <site…|all> [--dry-run]
                   the family's sites to their Workers: app gen verse md world
                   (--dry-run assembles and resolves each config, publishes nothing)";
pub const ASSEMBLE_USAGE: &str = "  assemble md | world [--all]
                   put a site together from the crates (world --all: and the
                   curated worlds' binaries from the assets checkout)";
pub const ADD_WORLD_USAGE: &str = "  add-world <world.json> <name> --from <dir>…
                   a world into localgpt.world's gallery, its models shrunk for the web";

/// A site's name, its folder and where it serves, in the family strip's
/// order (what `all` deploys in).
/// A site's name, its folder, and where it serves.
type Site = (&'static str, &'static str, &'static str);

const SITES: [Site; 5] = [
    ("app", "website", "https://localgpt.app"),
    ("gen", "website-gen", "https://gen.localgpt.app"),
    ("verse", "website-verse", "https://verse.localgpt.app"),
    ("md", "website-md", "https://localgpt.md"),
    ("world", "website-world", "https://localgpt.world"),
];

fn npx() -> &'static str {
    if cfg!(windows) { "npx.cmd" } else { "npx" }
}

/// The sites the arguments name, in order, and whether it is a dry run.
fn deploy_args(args: &[String]) -> Result<(Vec<&'static Site>, bool), String> {
    let mut sites = Vec::new();
    let mut dry_run = false;
    for arg in args {
        match arg.as_str() {
            "--dry-run" => dry_run = true,
            "all" => sites = SITES.iter().collect(),
            flag if flag.starts_with('-') => return Err(format!("unexpected option {flag:?}")),
            name => {
                let site = SITES
                    .iter()
                    .find(|(site, _, _)| *site == name)
                    .ok_or_else(|| {
                        format!("unknown site {name:?} (app gen verse md world, or all)")
                    })?;
                if !sites.contains(&site) {
                    sites.push(site);
                }
            }
        }
    }
    if sites.is_empty() {
        return Err(format!(
            "usage: cargo xtask deploy <site…|all> [--dry-run]\n{DEPLOY_USAGE}"
        ));
    }
    Ok((sites, dry_run))
}

/// `cargo xtask deploy`. There is one environment and it is production, so
/// a deploy of more than one site prints the plan and asks once, at a
/// terminal, before the first upload. A dry run needs no login.
pub fn deploy(root: &Path, args: &[String]) -> Result<(), String> {
    let (sites, dry_run) = deploy_args(args)?;
    if !dry_run {
        let whoami = Command::new(npx())
            .args(["--yes", "wrangler", "whoami"])
            .current_dir(root)
            .output()
            .map_err(|e| format!("npx would not start (install node 22): {e}"))?;
        let said = format!(
            "{}{}",
            String::from_utf8_lossy(&whoami.stdout),
            String::from_utf8_lossy(&whoami.stderr)
        );
        if !said.contains("Account Name") {
            return Err("Not logged in to Cloudflare. Run: npx wrangler login".into());
        }
    }
    println!("Deploying:");
    for (site, dir, host) in &sites {
        println!("  {site:<6} {:<15} -> {host}", format!("{dir}/"));
    }
    if dry_run {
        println!("  mode:  DRY RUN (assembles, publishes nothing)");
    }
    println!();
    if !dry_run && sites.len() > 1 && std::io::stdin().is_terminal() {
        print!("Publish these {} sites now? [y/N] ", sites.len());
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let mut answer = String::new();
        let _ = std::io::stdin().read_line(&mut answer);
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            return Err("Aborted.".into());
        }
        println!();
    }
    for (site, dir, _) in &sites {
        println!("==> {site} ({dir})");
        let folder = root.join(dir);
        match *site {
            "app" => run_in(&folder, "zola", &["build"])?,
            "md" => assemble_md(root)?,
            "world" => assemble_world(root, true)?,
            _ => {}
        }
        let mut wrangler = vec!["--yes", "wrangler", "deploy"];
        if dry_run {
            wrangler.push("--dry-run");
        }
        run_in(&folder, npx(), &wrangler)?;
        println!();
    }
    if !dry_run {
        println!("Deployed. Verify:");
        for (_, _, host) in &sites {
            println!("  curl -sI {host} | head -1");
        }
    }
    Ok(())
}

/// `cargo xtask assemble md | world [--all]`.
pub fn assemble(root: &Path, args: &[String]) -> Result<(), String> {
    let all = args.iter().any(|a| a == "--all");
    match args.first().map(String::as_str) {
        Some("md") => assemble_md(root),
        Some("world") => assemble_world(root, all),
        _ => Err(format!(
            "usage: cargo xtask assemble md | world [--all]\n{ASSEMBLE_USAGE}"
        )),
    }
}

/// localgpt.md: the sample screenplays (the same one as Fountain, Final
/// Draft, PDF and Markdown script pages, which draft to the same board byte
/// for byte) and the WASM draft, built by wasm-pack from
/// `crates/previs-web` (which skips when it is up to date). Nothing it
/// copies is tracked: the page needs no viewer of its own.
fn assemble_md(root: &Path) -> Result<(), String> {
    let site = root.join("website-md");
    let samples = site.join("samples");
    std::fs::create_dir_all(&samples).map_err(|e| format!("{}: {e}", samples.display()))?;
    for sample in [
        "cold-eggs.fountain",
        "cold-eggs.fdx",
        "cold-eggs.pdf",
        "cold-eggs.md",
    ] {
        let from = root.join("crates/previs/samples").join(sample);
        std::fs::copy(&from, samples.join(sample))
            .map_err(|e| format!("{}: {e}", from.display()))?;
    }
    if Command::new("wasm-pack").arg("--version").output().is_err() {
        eprintln!("wasm-pack not found: wasm/ left as is (CI builds it separately)");
        return Ok(());
    }
    run_in(
        root,
        "wasm-pack",
        &[
            "build",
            "crates/previs-web",
            "--target",
            "web",
            "--out-dir",
            "../../website-md/wasm",
        ],
    )?;
    println!("assembled");
    Ok(())
}

/// localgpt.world: the one web renderer (and the ext-cinematography module
/// it imports as `./cinematography.js`) and the conformance scenes, from
/// the crates that own them in this same commit, so they cannot drift; with
/// `all`, the curated worlds' shrunk GLBs, music and posters from the assets
/// checkout (`LOCALGPT_WORLD_ASSETS`, else a sibling), derivatives that
/// gltf-transform made and that are stored there rather than rebuilt here.
/// Without them the conformance worlds still render; the curated ones 404
/// their meshes.
fn assemble_world(root: &Path, all: bool) -> Result<(), String> {
    let site = root.join("website-world");
    let viewer = site.join("viewer");
    std::fs::create_dir_all(&viewer).map_err(|e| format!("{}: {e}", viewer.display()))?;
    for file in ["world-viewer.js", "cinematography.js"] {
        let from = root.join("crates/world-export/js").join(file);
        std::fs::copy(&from, viewer.join(file)).map_err(|e| format!("{}: {e}", from.display()))?;
        println!("  viewer/{file} <- crates/world-export");
    }
    let conformance = root.join("crates/world-types/conformance");
    let worlds = site.join("worlds");
    std::fs::create_dir_all(&worlds).map_err(|e| format!("{}: {e}", worlds.display()))?;
    let mut scenes = 0;
    for entry in std::fs::read_dir(&conformance)
        .map_err(|e| format!("{}: {e}", conformance.display()))?
        .flatten()
    {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "json") {
            std::fs::copy(&path, worlds.join(entry.file_name()))
                .map_err(|e| format!("{}: {e}", path.display()))?;
            scenes += 1;
        }
    }
    println!("  worlds/ <- {scenes} conformance scenes");
    if copy_dir(&conformance.join("assets"), &worlds.join("assets"))? {
        println!("  worlds/assets/ <- conformance assets");
    }
    if all {
        let mut roots: Vec<PathBuf> = std::env::var_os("LOCALGPT_WORLD_ASSETS")
            .map(PathBuf::from)
            .into_iter()
            .collect();
        roots.extend(crate::assets::checkouts(root));
        let assets = roots
            .into_iter()
            .find(|dir| dir.join("web/worlds").is_dir())
            .ok_or("no assets checkout with web/worlds/: set LOCALGPT_WORLD_ASSETS")?;
        if copy_dir(&assets.join("web/worlds/assets"), &worlds.join("assets"))? {
            println!("  worlds/assets/ <- web derivatives");
        }
        if copy_dir(&assets.join("web/worlds/posters"), &worlds.join("posters"))? {
            println!("  worlds/posters/");
        }
        // An un-pulled LFS object is a small text pointer, which the
        // viewer's loader reports as a corrupt model rather than a missing
        // one: say so plainly.
        let models = worlds.join("assets/models");
        for entry in std::fs::read_dir(&models).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let glb = entry.path().join(format!("{name}.glb"));
            if let Ok(meta) = std::fs::metadata(&glb)
                && meta.len() < 1024
            {
                return Err(format!(
                    "{name}.glb is {} bytes: an LFS pointer. Run `git lfs pull` in the assets checkout and assemble again.",
                    meta.len()
                ));
            }
        }
    }
    println!(
        "{}",
        if all {
            "assembled (full)"
        } else {
            "assembled (in-repo sources only)"
        }
    );
    Ok(())
}

/// `from` copied into `to`, file by file, over what is there; false when
/// there is no `from`.
fn copy_dir(from: &Path, to: &Path) -> Result<bool, String> {
    if !from.is_dir() {
        return Ok(false);
    }
    std::fs::create_dir_all(to).map_err(|e| format!("{}: {e}", to.display()))?;
    for entry in std::fs::read_dir(from)
        .map_err(|e| format!("{}: {e}", from.display()))?
        .flatten()
    {
        let (path, target) = (entry.path(), to.join(entry.file_name()));
        if path.is_dir() {
            copy_dir(&path, &target)?;
        } else {
            std::fs::copy(&path, &target).map_err(|e| format!("{}: {e}", path.display()))?;
        }
    }
    Ok(true)
}

const GLTF_TRANSFORM: &str = "@gltf-transform/cli@4.5.0";

/// `cargo xtask add-world <world.json> <name> --from <dir>…`: the world's
/// manifest to `worlds/<name>.json` and every asset it references into
/// `worlds/assets/`, glTF models shrunk for the web on the way (textures
/// to 512 px, then weld, simplify and prune), kept core glTF so the viewer
/// and `localgpt-gen --world` load them without decoders. An asset path is
/// relative to the format's `assets/` folder and looked up under the
/// `--from` roots in order; one already on the site is kept, so worlds
/// share models.
pub fn add_world(root: &Path, args: &[String]) -> Result<(), String> {
    let (mut roots, mut positional) = (Vec::new(), Vec::new());
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--from" {
            roots.push(PathBuf::from(iter.next().ok_or("--from needs a folder")?));
        } else {
            positional.push(arg.as_str());
        }
    }
    let [source, name] = positional[..] else {
        return Err(format!(
            "usage: cargo xtask add-world <world.json> <name> --from <dir>…\n{ADD_WORLD_USAGE}"
        ));
    };
    if roots.is_empty()
        || name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("a name of a-z, 0-9 and -, and at least one --from".into());
    }
    let text = std::fs::read_to_string(source).map_err(|e| format!("{source}: {e}"))?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{source}: {e}"))?;
    let entities = manifest["entities"]
        .as_array()
        .ok_or_else(|| format!("{source}: not a world manifest"))?;
    let assets = referenced(&manifest);
    let worlds = root.join("website-world/worlds");
    let (mut before, mut after) = (0u64, 0u64);
    let mb = |bytes: u64| format!("{:.1} MB", bytes as f64 / 1e6);
    for asset in &assets {
        let rel = Path::new(asset);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(format!("unsafe asset path: {asset}"));
        }
        let dest = worlds.join("assets").join(rel);
        if dest.exists() {
            println!("kept    {asset}");
            continue;
        }
        let from = roots
            .iter()
            .map(|root| root.join(rel))
            .find(|path| path.exists())
            .ok_or_else(|| format!("{asset}: not found under the --from folders"))?;
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        let size = std::fs::metadata(&from).map(|m| m.len()).unwrap_or(0);
        before += size;
        let model = rel
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("glb") || ext.eq_ignore_ascii_case("gltf"));
        let copy = || {
            std::fs::copy(&from, &dest)
                .map(|_| ())
                .map_err(|e| format!("{asset}: {e}"))
        };
        if model {
            let (from_s, dest_s) = (from.to_string_lossy(), dest.to_string_lossy());
            run_quiet(
                npx(),
                &[
                    "-y",
                    GLTF_TRANSFORM,
                    "resize",
                    &from_s,
                    &dest_s,
                    "--width",
                    "512",
                    "--height",
                    "512",
                ],
            )?;
            run_quiet(
                npx(),
                &[
                    "-y",
                    GLTF_TRANSFORM,
                    "optimize",
                    &dest_s,
                    &dest_s,
                    "--compress",
                    "false",
                    "--texture-compress",
                    "false",
                    "--instance",
                    "false",
                    "--palette",
                    "false",
                    "--simplify-ratio",
                    "0.1",
                    "--simplify-error",
                    "0.004",
                ],
            )?;
            // A small model can come out larger than it went in: keep it.
            if std::fs::metadata(&dest)
                .map(|m| m.len())
                .unwrap_or(u64::MAX)
                >= size
            {
                copy()?;
            }
        } else {
            copy()?;
        }
        let now = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
        after += now;
        println!("added   {asset} ({} -> {})", mb(size), mb(now));
    }
    std::fs::write(worlds.join(format!("{name}.json")), &text)
        .map_err(|e| format!("{name}.json: {e}"))?;
    println!(
        "worlds/{name}.json: {} entities, {} assets{}",
        entities.len(),
        assets.len(),
        if before > 0 {
            format!(", new assets {} -> {}", mb(before), mb(after))
        } else {
            String::new()
        }
    );
    Ok(())
}

/// The assets a world references: its entities' meshes and sounds, and its
/// soundtrack; sorted, each once.
fn referenced(manifest: &serde_json::Value) -> Vec<String> {
    let mut assets = std::collections::BTreeSet::new();
    for entity in manifest["entities"].as_array().into_iter().flatten() {
        if let Some(path) = entity["mesh_asset"]["path"].as_str() {
            assets.insert(path.to_string());
        }
        if let Some(path) = entity["audio"]["source"]["File"]["path"].as_str() {
            assets.insert(path.to_string());
        }
    }
    if let Some(path) = manifest["soundtrack"]["path"].as_str() {
        assets.insert(path.to_string());
    }
    assets.into_iter().collect()
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

fn run_quiet(program: &str, args: &[&str]) -> Result<(), String> {
    let out = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("{program} would not start: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "{program} {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn sites_by_name_in_the_strips_order() {
        let (sites, dry) = deploy_args(&args(&["all", "--dry-run"])).unwrap();
        let names: Vec<&str> = sites.iter().map(|(name, _, _)| *name).collect();
        assert_eq!(names, ["app", "gen", "verse", "md", "world"]);
        assert!(dry);
        let (sites, dry) = deploy_args(&args(&["world", "md", "world"])).unwrap();
        assert_eq!(sites.len(), 2, "each site once");
        assert!(!dry);
        assert!(deploy_args(&args(&["blog"])).is_err());
        assert!(deploy_args(&args(&[])).is_err());
    }

    #[test]
    fn a_worlds_assets_are_its_meshes_sounds_and_soundtrack() {
        let manifest = serde_json::json!({
            "entities": [
                {"mesh_asset": {"path": "models/rock/rock.glb"}},
                {"audio": {"source": {"File": {"path": "sounds/wind.ogg"}}}},
                {"mesh_asset": {"path": "models/rock/rock.glb"}},
                {"name": "bare"}
            ],
            "soundtrack": {"path": "music/amber-drift.mp3"}
        });
        assert_eq!(
            referenced(&manifest),
            [
                "models/rock/rock.glb",
                "music/amber-drift.mp3",
                "sounds/wind.ogg"
            ]
        );
    }
}
