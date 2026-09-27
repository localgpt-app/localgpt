// Add a world to the site: copy its manifest to worlds/<name>.json
// and every asset it references into worlds/assets/, shrinking glTF
// models for the web on the way (textures to 512 px, then weld, simplify and
// prune). The copies stay core glTF (no compression extensions), so the
// viewer and `localgpt-gen --world` load them without decoders.
//
//   node scripts/add-world.mjs <world.json> <name> --from <dir> [--from <dir>]...
//
// Asset paths in a manifest are relative to its assets/ folder (the format's
// convention). Each is looked up under the --from roots in order; for a Verse
// export: --from ../localgpt-verse/assets --from ../localgpt-verse-assets.
// Assets already on the site are kept, so worlds share models.
import { execFileSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';

const GLTF_TRANSFORM = '@gltf-transform/cli@4.5.0';
const worlds = fileURLToPath(new URL('../worlds/', import.meta.url));

const args = process.argv.slice(2);
const roots = [];
const positional = [];
for (let i = 0; i < args.length; i += 1) {
  if (args[i] === '--from') roots.push(args[(i += 1)]);
  else positional.push(args[i]);
}
const [source, name] = positional;
if (!source || !name || !/^[a-z0-9-]+$/.test(name) || roots.length === 0) {
  console.error('usage: node scripts/add-world.mjs <world.json> <name: a-z0-9-> --from <dir>...');
  process.exit(2);
}

const text = readFileSync(source, 'utf8');
const manifest = JSON.parse(text);
if (!Array.isArray(manifest.entities)) throw new Error(`${source}: not a world manifest`);

const assets = new Set();
for (const entity of manifest.entities) {
  if (entity.mesh_asset?.path) assets.add(entity.mesh_asset.path);
  const file = entity.audio?.source?.File;
  if (file?.path) assets.add(file.path);
}
if (manifest.soundtrack?.path) assets.add(manifest.soundtrack.path);

const mb = (bytes) => `${(bytes / 1e6).toFixed(1)} MB`;
let before = 0;
let after = 0;
for (const asset of [...assets].sort()) {
  const rel = normalize(asset);
  if (rel.startsWith('..') || rel.startsWith('/')) throw new Error(`unsafe asset path: ${asset}`);
  const dest = join(worlds, 'assets', rel);
  if (existsSync(dest)) {
    console.log(`kept    ${asset}`);
    continue;
  }
  const from = roots.map((root) => join(root, rel)).find((p) => existsSync(p));
  if (!from) throw new Error(`${asset}: not found under ${roots.join(', ')}`);
  mkdirSync(dirname(dest), { recursive: true });
  const size = statSync(from).size;
  before += size;
  if (/\.gl(b|tf)$/i.test(rel)) {
    const run = (...command) => execFileSync('npx', ['-y', GLTF_TRANSFORM, ...command], { stdio: 'ignore' });
    run('resize', from, dest, '--width', '512', '--height', '512');
    run('optimize', dest, dest, '--compress', 'false', '--texture-compress', 'false',
      '--instance', 'false', '--palette', 'false',
      '--simplify-ratio', '0.1', '--simplify-error', '0.004');
    // Small models can come out larger than they went in; keep the original.
    if (statSync(dest).size >= size) copyFileSync(from, dest);
  } else {
    copyFileSync(from, dest);
  }
  after += statSync(dest).size;
  console.log(`added   ${asset} (${mb(size)} → ${mb(statSync(dest).size)})`);
}

writeFileSync(join(worlds, `${name}.json`), text);
console.log(`worlds/${name}.json: ${manifest.entities.length} entities, ${assets.size} assets`
  + (before ? `, new assets ${mb(before)} → ${mb(after)}` : ''));
