# MCP Registry Submission Guide — localgpt-gen

This document explains how to submit `localgpt-gen` to the
[MCP Registry](https://registry.modelcontextprotocol.io) once ready.

The `server.json` in this directory is the registry manifest.
Review it, complete the TODOs below, then follow the steps.

---

## Status

| Item | Status |
|---|---|
| `server.json` manifest | ✅ Ready (see TODOs below) |
| `mcp-publisher` CLI installed | ⬜ |
| OCI image published to GHCR | ⬜ |
| GitHub namespace verified | ⬜ (automatic via OAuth) |

---

## TODOs Before Submitting

### 1. Publish a Docker image to GHCR

The MCP Registry does not yet support `crates.io` as a package source
(`npm`, `pypi`, `nuget`, `oci`, `mcpb` are the supported types).
The most practical path for a Rust binary is an OCI image via
GitHub Container Registry (GHCR).

Build and push:

```bash
# From the repo root
docker build -t ghcr.io/localgpt-app/localgpt-gen:0.3.5 \
             -t ghcr.io/localgpt-app/localgpt-gen:latest \
             -f Dockerfile .

# Authenticate
echo $GITHUB_TOKEN | docker login ghcr.io -u localgpt-app --password-stdin

# Push
docker push ghcr.io/localgpt-app/localgpt-gen:0.3.5
docker push ghcr.io/localgpt-app/localgpt-gen:latest
```

The `packages[0].identifier` in `server.json` is already set to
`ghcr.io/localgpt-app/localgpt-gen` — no change needed once the image is up.

**Alternative:** If you want to request native `cargo install` support in the
registry, open an issue at https://github.com/modelcontextprotocol/registry.
It requires a PR adding a new validator for the `cargo` registryType.

### 2. Add the package name to the Docker image label (namespace verification)

For OCI images the registry verifies ownership via the container label.
Add this to the Dockerfile (or to the `docker build` command with `--label`):

```dockerfile
LABEL org.opencontainers.image.source="https://github.com/localgpt-app/localgpt"
LABEL io.modelcontextprotocol.name="io.github.localgpt-app/localgpt-gen"
```

### 3. Verify the icon URL

Update `icons[0].src` in `server.json` if the URL differs:

```json
"icons": [
  {
    "src": "https://localgpt.app/img/localgpt-gen-icon.png",
    "mimeType": "image/png",
    "sizes": "512x512"
  }
]
```

If no icon is hosted yet, remove the `icons` array entirely for now
(it is optional) and add it later.

---

## Submission Steps

### 1. Install `mcp-publisher`

```bash
# macOS/Linux via Homebrew
brew install modelcontextprotocol/tap/mcp-publisher

# Or via the install script
curl -fsSL https://static.modelcontextprotocol.io/install.sh | sh
```

### 2. Validate the manifest

From this directory (`crates/gen/`):

```bash
mcp-publisher validate
```

Fix any schema errors before proceeding.

### 3. Authenticate via GitHub OAuth

```bash
mcp-publisher login github
```

This opens a browser. Log in as `localgpt-app` (or a member of that org).
The `io.github.localgpt-app/*` namespace is automatically authorized.

For CI/CD (GitHub Actions), use OIDC instead:

```yaml
- name: Publish to MCP Registry
  uses: modelcontextprotocol/publish-action@v1
  with:
    namespace: io.github.localgpt-app
```

### 4. Publish

```bash
mcp-publisher publish
```

The `server.json` in the current directory is picked up automatically.

---

## What the Registry Entry Will Show

**Name:** `io.github.localgpt-app/localgpt-gen`
**Title:** LocalGPT Gen
**Description:** AI-driven 3D world builder: Bevy visuals, procedural audio, and entity behaviors.
**Version:** 0.3.5
**Transport:** stdio
**Install:** `docker run --rm -i ghcr.io/localgpt-app/localgpt-gen:0.3.5 mcp-server`
**Docs:** https://localgpt.app

---

## What `localgpt-gen mcp-server` exposes

When an MCP client connects, the server advertises **83 tools** across three capability pillars:

### Visual — Bevy 3D Engine

Real-time 3D scene rendering via Bevy 0.18 with an interactive viewport.

| Category | Tools |
|---|---|
| Scene query | `gen_scene_info`, `gen_entity_info`, `gen_screenshot` |
| Spawn | `gen_spawn_primitive`, `gen_spawn_batch`, `gen_spawn_mesh`, `gen_load_gltf` |
| Modify | `gen_modify_entity`, `gen_modify_batch`, `gen_delete_entity`, `gen_delete_batch` |
| Camera | `gen_set_camera`, `gen_set_camera_mode` |
| Lighting | `gen_set_light`, `gen_set_environment`, `gen_set_sky` |
| Export | `gen_export_screenshot`, `gen_export_gltf`, `gen_export_html` |
| World skills | `gen_save_world`, `gen_load_world`, `gen_export_world`, `gen_clear_scene` |
| History | `gen_undo`, `gen_redo`, `gen_undo_info` |
| WorldGen pipeline | `gen_plan_layout`, `gen_apply_blockout`, `gen_populate_region`, `gen_evaluate_scene`, `gen_auto_refine`, `gen_build_navmesh`, `gen_validate_navigability`, `gen_edit_navmesh`, `gen_modify_blockout`, `gen_bulk_modify`, `gen_set_tier`, `gen_set_role`, `gen_regenerate`, `gen_render_depth`, `gen_preview_world` |
| AI asset gen | `gen_generate_asset`, `gen_asset_status`, `gen_list_assets` |
| Experiments | `gen_queue_experiment`, `gen_list_experiments`, `gen_experiment_status` |
| Terrain | `gen_add_terrain`, `gen_add_water`, `gen_add_path`, `gen_add_foliage` |

### Audio — FunDSP Procedural Synthesis

Algorithmic soundscapes and spatial emitters synthesized in real time (no samples).

| Category | Tools |
|---|---|
| Ambience | `gen_set_ambience` (wind, rain, forest, ocean, cave, stream, silence) |
| Emitters | `gen_audio_emitter`, `gen_modify_audio` |
| Status | `gen_audio_info` |

### Behavior — Entity Logic & Interaction

Data-driven animations, NPCs, physics, and interaction logic.

| Category | Tools |
|---|---|
| Behaviors | `gen_add_behavior`, `gen_remove_behavior`, `gen_list_behaviors`, `gen_pause_behaviors` |
| Characters | `gen_spawn_player`, `gen_set_spawn_point`, `gen_add_npc`, `gen_set_npc_dialogue` |
| Interaction | `gen_add_trigger`, `gen_add_teleporter`, `gen_add_collectible`, `gen_add_door`, `gen_link_entities` |
| Physics | `gen_set_physics`, `gen_add_collider`, `gen_add_joint`, `gen_add_force`, `gen_set_gravity` |
| In-world UI | `gen_add_sign`, `gen_add_hud`, `gen_add_label`, `gen_add_tooltip`, `gen_add_notification` |

### Core — Memory & Web

| Category | Tools |
|---|---|
| Memory | `memory_search`, `memory_get`, `memory_save`, `memory_log` |
| Web | `web_fetch`, `web_search` |

---

## Supported AI Clients

| Client | Config |
|---|---|
| Claude CLI | `claude mcp add localgpt-gen -- localgpt-gen mcp-server` |
| Gemini CLI | `gemini mcp add --name localgpt-gen -- localgpt-gen mcp-server` |
| Codex CLI | Add to `~/.codex/config.yaml` under `mcp.servers` |
| VS Code | Add to `.vscode/mcp.json` |
| Zed | Add to `~/.config/zed/settings.json` under `context_servers` |
| Cursor | Add to `.cursor/mcp.json` |
| Windsurf | Add to `~/.codeium/windsurf/mcp_config.json` |

See https://localgpt.app/docs/gen/mcp-server for full configuration examples.
