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
                   gallery packages' assets from the assets checkout)";
pub const ADD_WORLD_USAGE: &str = "  add-world <world.json|dir.world> <name> --from <dir>… [--keep]
                   a world into localgpt.world's gallery as a .world package, its
                   assets content-addressed and its models shrunk (--keep: as they are)";

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
/// the crates that own them in this same commit, so they cannot drift. The
/// gallery's worlds are `.world` packages tracked beside them
/// (`worlds/<name>.world/`: the manifest and its `package.json`); with
/// `all`, each package's content-addressed assets and the posters come from
/// the assets checkout (`LOCALGPT_WORLD_ASSETS`, else a sibling),
/// derivatives gltf-transform made that are stored there rather than rebuilt
/// here. Every package whose assets are all present is then zipped — the
/// format's transport form, what the page offers to download. Without the
/// checkout the conformance scenes and the primitives-only packages still
/// render and zip; the others wait for their assets.
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
    let packages = gallery_packages(&worlds)?;
    if all {
        let assets = assets_checkout(root)?;
        for package in &packages {
            let name = package_name(package);
            let from = assets.join(format!("web/worlds/{name}.world/assets"));
            if copy_dir(&from, &package.join("assets"))? {
                println!("  worlds/{name}.world/assets/ <- the assets checkout");
            }
        }
        if copy_dir(&assets.join("web/worlds/posters"), &worlds.join("posters"))? {
            println!("  worlds/posters/");
        }
        // An un-pulled LFS object is a small text pointer, which the
        // viewer's loader reports as a corrupt model rather than a missing
        // one: say so plainly.
        for package in &packages {
            let assets = package.join("assets");
            for entry in std::fs::read_dir(&assets).into_iter().flatten().flatten() {
                let path = entry.path();
                let media = path
                    .extension()
                    .is_some_and(|ext| ext == "glb" || ext == "mp3" || ext == "ogg");
                if media
                    && let Ok(meta) = std::fs::metadata(&path)
                    && meta.len() < 1024
                {
                    return Err(format!(
                        "{} is {} bytes: an LFS pointer. Run `git lfs pull` in the assets checkout and assemble again.",
                        path.strip_prefix(&site).unwrap_or(&path).display(),
                        meta.len()
                    ));
                }
            }
        }
    }
    let (mut zipped, mut waiting) = (0, Vec::new());
    for package in &packages {
        if package_zip(package)? {
            zipped += 1;
        } else {
            waiting.push(package_name(package));
        }
    }
    println!(
        "  {zipped} of {} packages zipped{}",
        packages.len(),
        if waiting.is_empty() {
            String::new()
        } else {
            format!(" (waiting for assets: {})", waiting.join(", "))
        }
    );
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

/// The assets checkout holding the gallery's derivatives:
/// `LOCALGPT_WORLD_ASSETS`, else a sibling under any name it has had.
fn assets_checkout(root: &Path) -> Result<PathBuf, String> {
    let mut roots: Vec<PathBuf> = std::env::var_os("LOCALGPT_WORLD_ASSETS")
        .map(PathBuf::from)
        .into_iter()
        .collect();
    roots.extend(crate::assets::checkouts(root));
    roots
        .into_iter()
        .find(|dir| dir.join("web/worlds").is_dir())
        .ok_or_else(|| "no assets checkout with web/worlds/: set LOCALGPT_WORLD_ASSETS".into())
}

/// The gallery's packages: every `worlds/<name>.world/` holding a
/// manifest, sorted by name.
fn gallery_packages(worlds: &Path) -> Result<Vec<PathBuf>, String> {
    let mut packages: Vec<PathBuf> = std::fs::read_dir(worlds)
        .map_err(|e| format!("{}: {e}", worlds.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path.extension().is_some_and(|ext| ext == "world")
                && path.join(MANIFEST).is_file()
        })
        .collect();
    packages.sort();
    Ok(packages)
}

/// `worlds/<name>.world` → `<name>`.
fn package_name(package: &Path) -> String {
    package
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The package's transport form, `worlds/<name>.world.zip`, once every
/// asset its manifest references is in the package: `<name>.world/` at the
/// archive's root, its files sorted and stored (the assets are compressed
/// media already) with one fixed timestamp, so the same package zips to the
/// same bytes. A package still waiting for its assets gets no zip, and a
/// stale one goes. True when the zip was written.
fn package_zip(package: &Path) -> Result<bool, String> {
    use std::fs::File;
    use zip::CompressionMethod;
    use zip::ZipWriter;
    use zip::write::SimpleFileOptions;

    let manifest = read_manifest(&package.join(MANIFEST))?;
    let zip_path = package.with_extension("world.zip");
    let assets = package.join("assets");
    if referenced(&manifest)
        .iter()
        .any(|asset| !assets.join(asset).is_file())
    {
        let _ = std::fs::remove_file(&zip_path);
        return Ok(false);
    }
    let mut files = Vec::new();
    collect_files(package, &mut files)?;
    files.sort();
    let dir = package
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let zip_error = |e: zip::result::ZipError| format!("{}: {e}", zip_path.display());
    let file = File::create(&zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut zip = ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(zip::DateTime::default());
    for path in &files {
        let rel = path
            .strip_prefix(package)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        zip.start_file(format!("{dir}/{rel}"), options)
            .map_err(zip_error)?;
        let mut source = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        std::io::copy(&mut source, &mut zip).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    zip.finish().map_err(zip_error)?;
    Ok(true)
}

/// Every file under `dir`, recursively; hidden entries (an app's `.live/`,
/// a Finder's `.DS_Store`) are not part of a package.
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
    {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn read_manifest(path: &Path) -> Result<serde_json::Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let manifest: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if !manifest["entities"].is_array() {
        return Err(format!("{}: not a world manifest", path.display()));
    }
    Ok(manifest)
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

/// The world now, the one file a viewer reads.
const MANIFEST: &str = openworldformat::package::MANIFEST;
/// The package's metadata and integrity record (spec/package.md).
const PACKAGE_JSON: &str = "package.json";
/// A material's texture slots, as the manifest spells them.
const TEXTURE_FIELDS: [&str; 4] = [
    "base_color_texture",
    "metallic_roughness_texture",
    "normal_map_texture",
    "emissive_texture",
];

/// Where a referenced asset went: its content-addressed name under the
/// package's `assets/`, and the hash that name promises.
#[derive(Debug, Clone, PartialEq)]
struct Stored {
    name: String,
    sha256: String,
}

/// `cargo xtask add-world <world.json|dir.world> <name> --from <dir>… [--keep]`:
/// the world into the gallery as the package `worlds/<name>.world/` — its
/// manifest in the format's canonical text, a `package.json` naming those
/// bytes, and every asset it references stored under `assets/` by content
/// hash (`<sha256>.<ext>`: the format's baseline for a published world, and
/// what `localgpt world verify` checks), the references rewritten to the
/// copies, a mesh reference carrying its hash. A glTF model is shrunk for
/// the web on the way (textures to 512 px, then weld, simplify and prune),
/// kept core glTF so the viewer and `localgpt-gen --world` load it without
/// decoders; `--keep` stores every file as it is, for sources that are
/// web-sized already. A reference is relative to the format's `assets/`
/// and looked up under the `--from` roots in order — for a `.world` source,
/// its own `assets/` first. The copies belong in the assets checkout under
/// `web/worlds/<name>.world/assets/`; the manifest and `package.json` are
/// tracked here.
pub fn add_world(root: &Path, args: &[String]) -> Result<(), String> {
    let (mut roots, mut positional, mut keep) = (Vec::new(), Vec::new(), false);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--from" => roots.push(PathBuf::from(iter.next().ok_or("--from needs a folder")?)),
            "--keep" => keep = true,
            other => positional.push(other),
        }
    }
    let [source, name] = positional[..] else {
        return Err(format!(
            "usage: cargo xtask add-world <world.json|dir.world> <name> --from <dir>… [--keep]\n{ADD_WORLD_USAGE}"
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
    let source = Path::new(source);
    let manifest_path = if source.is_dir() {
        roots.insert(0, source.join("assets"));
        source.join(MANIFEST)
    } else {
        source.to_path_buf()
    };
    let mut manifest = read_manifest(&manifest_path)?;
    let entities = manifest["entities"].as_array().map_or(0, Vec::len);
    let package = root.join(format!("website-world/worlds/{name}.world"));
    let assets_dir = package.join("assets");
    std::fs::create_dir_all(&assets_dir).map_err(|e| format!("{}: {e}", assets_dir.display()))?;
    let (mut before, mut after, mut added) = (0u64, 0u64, 0usize);
    let mb = |bytes: u64| format!("{:.1} MB", bytes as f64 / 1e6);
    let mut stored = std::collections::BTreeMap::new();
    for asset in referenced(&manifest) {
        let rel = Path::new(&asset);
        if rel.is_absolute()
            || rel
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(format!("unsafe asset path: {asset}"));
        }
        let from = roots
            .iter()
            .map(|root| root.join(rel))
            .find(|path| path.is_file())
            .ok_or_else(|| format!("{asset}: not found under the --from folders"))?;
        let size = std::fs::metadata(&from).map(|m| m.len()).unwrap_or(0);
        let model = rel
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("glb") || ext.eq_ignore_ascii_case("gltf"));
        // Shrink or copy into a staging file first: the name is the hash of
        // what comes out, and that is known only afterwards.
        let incoming = assets_dir.join(".incoming");
        let copy = || {
            std::fs::copy(&from, &incoming)
                .map(|_| ())
                .map_err(|e| format!("{asset}: {e}"))
        };
        if model && !keep {
            shrink_model(&from, &incoming)?;
            // A small model can come out larger than it went in: keep it.
            if std::fs::metadata(&incoming)
                .map(|m| m.len())
                .unwrap_or(u64::MAX)
                >= size
            {
                copy()?;
            }
        } else {
            copy()?;
        }
        let bytes = std::fs::read(&incoming).map_err(|e| format!("{asset}: {e}"))?;
        let sha256 = openworldformat::sha256_hex(&bytes);
        let copy_name = stored_name(&asset, &sha256);
        let dest = assets_dir.join(&copy_name);
        if dest.is_file() {
            std::fs::remove_file(&incoming).map_err(|e| format!("{asset}: {e}"))?;
            println!("kept    {asset} = {copy_name}");
        } else {
            std::fs::rename(&incoming, &dest).map_err(|e| format!("{asset}: {e}"))?;
            added += 1;
            before += size;
            after += bytes.len() as u64;
            println!(
                "added   {asset} -> {copy_name} ({} -> {})",
                mb(size),
                mb(bytes.len() as u64)
            );
        }
        stored.insert(
            asset,
            Stored {
                name: copy_name,
                sha256,
            },
        );
    }
    rewrite_refs(&mut manifest, &stored);
    let text = openworldformat::manifest_text(&manifest);
    std::fs::write(package.join(MANIFEST), &text)
        .map_err(|e| format!("{}: {e}", package.join(MANIFEST).display()))?;
    // package.json names the manifest's bytes; the record is rewritten only
    // when the world changed, so a rerun that changes nothing changes nothing.
    let sha256 = openworldformat::sha256_hex(text.as_bytes());
    let record = package.join(PACKAGE_JSON);
    let current: Option<openworldformat::session::SessionMeta> = std::fs::read(&record)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok());
    if current.and_then(|meta| meta.world_sha256) != Some(sha256.clone()) {
        let meta = package_meta(name, &manifest, sha256);
        let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
        std::fs::write(&record, format!("{json}\n"))
            .map_err(|e| format!("{}: {e}", record.display()))?;
    }
    println!(
        "worlds/{name}.world: {entities} entities, {} assets ({added} added){}",
        stored.len(),
        if before > 0 {
            format!(", {} -> {}", mb(before), mb(after))
        } else {
            String::new()
        }
    );
    Ok(())
}

/// A glTF model shrunk for the web: textures to 512 px, then weld, simplify
/// and prune, staying core glTF.
fn shrink_model(from: &Path, to: &Path) -> Result<(), String> {
    let (from_s, to_s) = (from.to_string_lossy(), to.to_string_lossy());
    run_quiet(
        npx(),
        &[
            "-y",
            GLTF_TRANSFORM,
            "resize",
            &from_s,
            &to_s,
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
            &to_s,
            &to_s,
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
    )
}

/// The content-addressed name of a stored copy: the hash, with the
/// source's extension in lowercase (`<sha256>.glb`); the hash alone when
/// the source had none.
fn stored_name(path: &str, sha256: &str) -> String {
    match Path::new(path).extension() {
        Some(ext) => format!("{sha256}.{}", ext.to_string_lossy().to_ascii_lowercase()),
        None => sha256.to_string(),
    }
}

/// Every reference in the manifest moved to its stored copy: a mesh
/// reference's path and its `sha256`, an audio emitter's file, a material's
/// textures, the soundtrack. A reference to nothing stored stays as it is.
fn rewrite_refs(
    manifest: &mut serde_json::Value,
    stored: &std::collections::BTreeMap<String, Stored>,
) {
    use serde_json::Value;
    let relocate = |slot: &mut Value| {
        if let Some(copy) = slot.as_str().and_then(|path| stored.get(path)) {
            *slot = Value::String(copy.name.clone());
        }
    };
    for entity in manifest["entities"].as_array_mut().into_iter().flatten() {
        if let Some(mesh) = entity.get_mut("mesh_asset").and_then(Value::as_object_mut)
            && let Some(copy) = mesh
                .get("path")
                .and_then(Value::as_str)
                .and_then(|path| stored.get(path))
        {
            let (name, sha256) = (copy.name.clone(), copy.sha256.clone());
            mesh.insert("path".into(), Value::String(name));
            mesh.insert("sha256".into(), Value::String(sha256));
        }
        if let Some(slot) = entity.pointer_mut("/audio/source/File/path") {
            relocate(slot);
        }
        for field in TEXTURE_FIELDS {
            if let Some(slot) = entity.pointer_mut(&format!("/material/{field}")) {
                relocate(slot);
            }
        }
    }
    if let Some(slot) = manifest.pointer_mut("/soundtrack/path") {
        relocate(slot);
    }
}

/// The package's `package.json`: the format's session metadata for a
/// package with no history yet — revision 0, the manifest's bytes named,
/// the app the gallery name says made it.
fn package_meta(
    name: &str,
    manifest: &serde_json::Value,
    world_sha256: String,
) -> openworldformat::session::SessionMeta {
    let title = manifest["meta"]["name"]
        .as_str()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(name);
    let mut meta = openworldformat::session::SessionMeta::new(title);
    meta.app = app_of(name);
    meta.world_sha256 = Some(world_sha256);
    meta.updated_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    meta
}

/// Which app made a gallery world, read off its name's prefix
/// (`gen-…`, `md-…`, `verse-…`); none for any other name.
fn app_of(name: &str) -> Option<String> {
    let prefix = name.split('-').next()?;
    matches!(prefix, "gen" | "md" | "verse").then(|| format!("localgpt-{prefix}"))
}

/// The assets a world references, relative to its `assets/`: its entities'
/// meshes, sounds and textures, and its soundtrack; sorted, each once.
fn referenced(manifest: &serde_json::Value) -> Vec<String> {
    let mut assets = std::collections::BTreeSet::new();
    for entity in manifest["entities"].as_array().into_iter().flatten() {
        if let Some(path) = entity["mesh_asset"]["path"].as_str() {
            assets.insert(path.to_string());
        }
        if let Some(path) = entity["audio"]["source"]["File"]["path"].as_str() {
            assets.insert(path.to_string());
        }
        for field in TEXTURE_FIELDS {
            if let Some(path) = entity["material"][field].as_str() {
                assets.insert(path.to_string());
            }
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
    fn a_worlds_assets_are_its_meshes_sounds_textures_and_soundtrack() {
        let manifest = serde_json::json!({
            "entities": [
                {"mesh_asset": {"path": "models/rock/rock.glb"}},
                {"audio": {"source": {"File": {"path": "sounds/wind.ogg"}}}},
                {"mesh_asset": {"path": "models/rock/rock.glb"}},
                {"material": {"base_color_texture": "textures/checker_albedo.png", "normal_map_texture": "textures/checker_normal.png"}},
                {"name": "bare"}
            ],
            "soundtrack": {"path": "music/amber-drift.mp3"}
        });
        assert_eq!(
            referenced(&manifest),
            [
                "models/rock/rock.glb",
                "music/amber-drift.mp3",
                "sounds/wind.ogg",
                "textures/checker_albedo.png",
                "textures/checker_normal.png",
            ]
        );
    }

    #[test]
    fn a_stored_copy_is_named_by_its_hash_and_the_sources_extension() {
        let sha = "ab".repeat(32);
        assert_eq!(
            stored_name("models/rock/rock.GLB", &sha),
            format!("{sha}.glb")
        );
        assert_eq!(
            stored_name("music/amber-drift.mp3", &sha),
            format!("{sha}.mp3")
        );
        assert_eq!(stored_name("README", &sha), sha);
    }

    #[test]
    fn references_move_to_their_stored_copies() {
        let mut manifest = serde_json::json!({
            "entities": [
                {"mesh_asset": {"path": "models/rock/rock.glb", "node": "trunk"}},
                {"audio": {"source": {"File": {"path": "sounds/wind.ogg", "looping": true}}}},
                {"material": {"base_color_texture": "textures/checker_albedo.png", "roughness": 0.5}},
                {"mesh_asset": {"path": "models/elsewhere.glb"}}
            ],
            "soundtrack": {"path": "music/amber-drift.mp3", "bpm": 92}
        });
        let copy = |name: &str, sha: &str| Stored {
            name: name.to_string(),
            sha256: sha.to_string(),
        };
        let stored = std::collections::BTreeMap::from([
            ("models/rock/rock.glb".to_string(), copy("1111.glb", "1111")),
            ("sounds/wind.ogg".to_string(), copy("2222.ogg", "2222")),
            (
                "textures/checker_albedo.png".to_string(),
                copy("3333.png", "3333"),
            ),
            (
                "music/amber-drift.mp3".to_string(),
                copy("4444.mp3", "4444"),
            ),
        ]);
        rewrite_refs(&mut manifest, &stored);
        assert_eq!(
            manifest["entities"][0]["mesh_asset"],
            serde_json::json!({"path": "1111.glb", "node": "trunk", "sha256": "1111"})
        );
        assert_eq!(
            manifest["entities"][1]["audio"]["source"]["File"]["path"],
            "2222.ogg"
        );
        assert_eq!(
            manifest["entities"][1]["audio"]["source"]["File"]["looping"],
            true
        );
        assert_eq!(
            manifest["entities"][2]["material"]["base_color_texture"],
            "3333.png"
        );
        assert_eq!(manifest["entities"][2]["material"]["roughness"], 0.5);
        assert_eq!(
            manifest["entities"][3]["mesh_asset"]["path"],
            "models/elsewhere.glb"
        );
        assert!(
            manifest["entities"][3]["mesh_asset"]
                .get("sha256")
                .is_none()
        );
        assert_eq!(manifest["soundtrack"]["path"], "4444.mp3");
        assert_eq!(manifest["soundtrack"]["bpm"], 92);
    }

    #[test]
    fn the_app_is_read_off_the_gallery_name() {
        assert_eq!(
            app_of("gen-basalt-lighthouse").as_deref(),
            Some("localgpt-gen")
        );
        assert_eq!(app_of("md-hello").as_deref(), Some("localgpt-md"));
        assert_eq!(
            app_of("verse-nightglass").as_deref(),
            Some("localgpt-verse")
        );
        assert_eq!(app_of("castle"), None);
        let meta = package_meta(
            "gen-basalt-lighthouse",
            &serde_json::json!({"meta": {"name": "Basalt Lighthouse"}, "entities": []}),
            "f".repeat(64),
        );
        assert_eq!(meta.name, "Basalt Lighthouse");
        assert_eq!(meta.app.as_deref(), Some("localgpt-gen"));
        assert_eq!((meta.base_revision, meta.head_revision), (0, 0));
        assert_eq!(meta.world_sha256.as_deref(), Some("f".repeat(64).as_str()));
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("xtask-sites-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_package_zips_to_the_same_bytes_once_its_assets_are_there() {
        let worlds = temp("zip");
        let package = worlds.join("gen-lamp.world");
        std::fs::create_dir_all(package.join("assets")).unwrap();
        let sha = "ab".repeat(32);
        std::fs::write(
            package.join(MANIFEST),
            format!(r#"{{"entities": [{{"id": 1, "mesh_asset": {{"path": "{sha}.glb", "sha256": "{sha}"}}}}]}}"#),
        )
        .unwrap();
        std::fs::write(package.join(PACKAGE_JSON), "{}\n").unwrap();
        // Waiting for its model: no zip, and a stale one goes.
        let zip = worlds.join("gen-lamp.world.zip");
        std::fs::write(&zip, b"stale").unwrap();
        assert!(!package_zip(&package).unwrap());
        assert!(!zip.exists(), "a stale zip is removed");
        // The model arrives: the zip is written, and written the same twice.
        std::fs::write(
            package.join("assets").join(format!("{sha}.glb")),
            b"glb bytes",
        )
        .unwrap();
        std::fs::write(package.join(".DS_Store"), b"not a package file").unwrap();
        assert!(package_zip(&package).unwrap());
        let first = std::fs::read(&zip).unwrap();
        assert!(package_zip(&package).unwrap());
        assert_eq!(first, std::fs::read(&zip).unwrap(), "deterministic bytes");
        let text = String::from_utf8_lossy(&first).into_owned();
        assert!(
            text.contains("gen-lamp.world/manifest.json"),
            "entries under the package's folder"
        );
        assert!(text.contains(&format!("gen-lamp.world/assets/{sha}.glb")));
        assert!(
            !text.contains(".DS_Store"),
            "hidden files are not part of a package"
        );
        assert_eq!(gallery_packages(&worlds).unwrap(), [package.clone()]);
        assert_eq!(package_name(&package), "gen-lamp");
        std::fs::remove_dir_all(&worlds).ok();
    }
}
