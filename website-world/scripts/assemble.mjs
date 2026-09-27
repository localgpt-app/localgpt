// Assemble the site tree before a check or a deploy.
//
// This replaces localgpt-world's `sync-viewer.sh`, which copied the viewer and
// the conformance worlds ACROSS repositories and so needed a human to remember
// to run it. On 2026-09-27 the published viewer was found 185 lines behind
// crates/world-export — the site had been rendering with an older renderer than
// the apps. Inside one repository those copies come from a sibling directory in
// the same commit, so they cannot drift; this script only moves bytes.
//
//   node scripts/assemble.mjs          # in-repo sources only (what CI needs)
//   node scripts/assemble.mjs --all    # also the curated worlds' binaries
//
// In-repo, always:
//   crates/world-export/js/world-viewer.js  -> viewer/world-viewer.js
//   crates/world-types/conformance/*.json   -> worlds/
//   crates/world-types/conformance/assets/  -> worlds/assets/
//
// From the assets checkout, with --all ($LOCALGPT_WORLD_ASSETS, else a sibling
// localgpt-world-assets or its former name localgpt-verse-assets):
//   web/worlds/assets/   -> worlds/assets/     (the shrunk GLBs and music)
//   web/worlds/posters/  -> worlds/posters/
// Those are derivatives that @gltf-transform produced, not copies of the pack,
// which is why they are stored there rather than rebuilt here. Without them the
// seven conformance worlds still render; the curated ones 404 their meshes.
import { cp, mkdir, readdir, stat } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const site = fileURLToPath(new URL('../', import.meta.url));
const repo = fileURLToPath(new URL('../../', import.meta.url));
const all = process.argv.includes('--all');

async function copyDir(from, to, label) {
  if (!existsSync(from)) return false;
  await mkdir(to, { recursive: true });
  await cp(from, to, { recursive: true, force: true });
  console.log(`  ${label}`);
  return true;
}

// 1. The one web renderer, from the crate that owns it.
const viewer = join(repo, 'crates/world-export/js/world-viewer.js');
if (!existsSync(viewer)) {
  console.error(`missing ${viewer} — run this from within the localgpt workspace`);
  process.exit(1);
}
await mkdir(join(site, 'viewer'), { recursive: true });
await cp(viewer, join(site, 'viewer/world-viewer.js'), { force: true });
console.log('  viewer/world-viewer.js <- crates/world-export');

// 2. The conformance scenes and their textures.
const conformance = join(repo, 'crates/world-types/conformance');
const scenes = (await readdir(conformance)).filter((f) => f.endsWith('.json'));
await mkdir(join(site, 'worlds'), { recursive: true });
for (const scene of scenes) {
  await cp(join(conformance, scene), join(site, 'worlds', scene), { force: true });
}
console.log(`  worlds/ <- ${scenes.length} conformance scenes`);
await copyDir(join(conformance, 'assets'), join(site, 'worlds/assets'), 'worlds/assets/ <- conformance assets');

// 3. The curated worlds' binaries, only when asked for.
if (all) {
  const candidates = [
    process.env.LOCALGPT_WORLD_ASSETS,
    join(repo, '../localgpt-world-assets'),
    join(repo, '../localgpt-verse-assets'),
  ].filter(Boolean);
  const root = candidates.find((dir) => existsSync(join(dir, 'web/worlds')));
  if (!root) {
    console.error('no assets checkout with web/worlds/ — set $LOCALGPT_WORLD_ASSETS');
    process.exit(1);
  }
  await copyDir(join(root, 'web/worlds/assets'), join(site, 'worlds/assets'), 'worlds/assets/ <- web derivatives');
  await copyDir(join(root, 'web/worlds/posters'), join(site, 'worlds/posters'), 'worlds/posters/');
  // An un-pulled LFS object is a ~130-byte text pointer, which the GLTFLoader
  // reports as a corrupt model rather than a missing one. Say so plainly.
  const models = join(site, 'worlds/assets/models');
  if (existsSync(models)) {
    for (const dir of await readdir(models)) {
      const glb = join(models, dir, `${dir}.glb`);
      if (existsSync(glb) && (await stat(glb)).size < 1024) {
        console.error(`${dir}.glb is ${(await stat(glb)).size} bytes — an LFS pointer.`);
        console.error('Run `git lfs pull` in the assets checkout and assemble again.');
        process.exit(1);
      }
    }
  }
}
console.log(all ? 'assembled (full)' : 'assembled (in-repo sources only)');
