---
sidebar_position: 14.7
---

# Headless Mode & Experiment Queue

LocalGPT Gen can generate worlds **without opening a window** — from the command line, through the heartbeat task runner, or via MCP. Combined with the memory system, this enables an async creative workflow: **queue experiments, generate overnight, explore results in the morning.**

## Why Headless?

Interactive mode requires you to sit and watch the AI build. That's great for one scene, but professional world design needs **variation and comparison** — 5 lighting variations of a village, 3 density levels of a forest, different palettes for the same layout.

Headless mode lets you:

- **Queue experiments** and walk away
- **Script generation** in CI/CD pipelines
- **Batch variations** with a single command
- **Compare results** via the in-app gallery

No competitor in the AI world-generation space offers asynchronous local-first generation with persistent creative memory.

## Quick Start

```bash
# Generate a single world (no window)
localgpt-gen headless --prompt "Build a cozy cabin in a snowy forest"

# Generate with a style hint
localgpt-gen headless \
  --prompt "Medieval village marketplace" \
  --style "Studio Ghibli, watercolor feel"

# Custom output directory
localgpt-gen headless \
  --prompt "Sci-fi space station" \
  --output skills/station/

# Override model and timeout
localgpt-gen headless \
  --prompt "Underwater coral reef" \
  --model claude-cli/opus \
  --timeout 600
```

Each run produces a complete world skill in `workspace/skills/` — the same format as interactive mode, loadable with `gen_load_world`.

## CLI Flags

```
localgpt-gen headless --prompt <TEXT> [OPTIONS]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--prompt` | *(required)* | Generation prompt |
| `--output` | auto-named | Output world skill directory |
| `--style` | — | Style hint prepended to prompt |
| `--model` | config default | Override LLM model |
| `--timeout` | 300 | Max generation time in seconds |
| `--screenshot` | true | Capture thumbnail after generation |
| `--screenshot-width` | 1280 | Thumbnail width |
| `--screenshot-height` | 720 | Thumbnail height |

### Exit Codes

| Code | Meaning |
|------|---------|
| 0 | World generated and saved successfully |
| 1 | Generation failed (LLM error, tool error, timeout) |
| 2 | Invalid arguments or configuration |
| 3 | GPU not available |

## Memory Integration

This is where headless generation gets powerful. The memory system turns isolated generations into an **evolving creative practice**.

### Before Generation

The headless agent searches memory for your style preferences, entity templates, and past experiment outcomes — then applies them as defaults.

```
User's past sessions taught the AI:
  - Preferred palette: warm earth tones
  - Lighting style: golden hour, soft directional
  - Tree design: 3-tier canopy, trunk 0.3x2.0x0.3

Headless prompt: "Build a forest clearing"
  → AI finds style preferences via memory_search
  → Applies warm palette, golden lighting automatically
  → Uses the saved tree template for consistency
```

### After Generation

The agent saves structured summaries to memory — not raw conversation dumps, but scene state and design decisions:

```
World: forest-clearing | Entities: 24 | Style: warm earth tones, golden hour
Key entities: 8 trees (3-tier canopy), 3 bushes, stone path, campfire
Design decisions: Used Ghibli Forest style from memory, added fireflies
```

### Memory Formats

The gen memory system uses three structured formats:

**Style preferences** — saved when you praise a particular aesthetic:
```markdown
## Style: Ghibli Forest
- Palette: earth tones (#8B7355, #D4A574, #556B2F)
- Lighting: warm amber point lights, soft directional from 45 deg
- Atmosphere: magical realism, cozy
- Tags: nature, warm, ghibli, fantasy
```

**Entity templates** — saved when you iterate on a design:
```markdown
## Entity: Glowing Lantern
- Shape: box 0.15x0.4x0.15
- Color: #FFD700, emissive: 2.0
- Light: point, color #FFB347, intensity 800, radius 8
- Behavior: bob amplitude 0.1, speed 0.5
```

**Experiment results** — saved after variation sets:
```markdown
## Experiment: Enchanted Forest Variations
- sparse-forest: 8 trees, open clearings
- medium-forest: 20 trees, winding paths ★ WINNER
- dense-forest: 45 trees, too dark
- Finding: Medium density with dappled light felt best
```

All three are searchable via `memory_search`. Ask the AI to "use my lantern design" or "make it like the forest from last week" and it finds the right memory.

## Experiment Queue

### Via HEARTBEAT.md

Add experiments to `HEARTBEAT.md` in natural language, and the heartbeat runner processes them automatically:

```markdown
## Gen Experiments

- [ ] Build an enchanted forest — sparse trees, open clearings
- [ ] Build an enchanted forest — dense canopy, dark atmosphere
- [ ] Medieval village with 3 lighting variations (variation: lighting = dawn, noon, sunset)
- [ ] Sci-fi corridor in my "neon cyberpunk" style (style: from memory)
```

**Variation syntax:** `(variation: <axis> = <value1>, <value2>, ...)` expands into N separate experiments. Each produces its own world skill.

**Style reference:** `(style: from memory)` tells the agent to search memory for the named style before generating.

### Via MCP Tools

External AI backends can queue experiments programmatically:

| Tool | Description |
|------|-------------|
| `gen_queue_experiment` | Queue a generation task with optional variations |
| `gen_list_experiments` | List experiments by status (pending/running/completed/failed) |
| `gen_experiment_status` | Get detailed status of a specific experiment |

```json
// Queue a variation set via MCP
{
  "name": "gen_queue_experiment",
  "arguments": {
    "prompt": "Medieval village",
    "name": "village",
    "variations": {
      "axis": "lighting",
      "values": ["dawn", "noon", "sunset"]
    }
  }
}
```

### Experiment Tracking

Experiments are tracked in an append-only JSONL file (`<state_dir>/gen-experiments.jsonl`). Each record includes:

- Experiment ID, prompt, style
- Status (pending → running → completed/failed)
- Output path, screenshot path, entity count
- Duration, timestamps, error message
- Variation group and axis/value

### One at a time

The heartbeat dispatches one gen experiment per tick and waits for its `localgpt-gen headless` process to exit, so experiments never overlap each other. Nothing keeps a headless run off the GPU while a Gen window is open, though; on a machine with little GPU memory, close the window while experiments run.

## Gallery

View all generated worlds with the in-app gallery:

- Press **G** in the Bevy window to toggle the gallery overlay
- Use `/gallery` in the terminal for a text listing
- Filter by name, tag, or source
- Click **Load** to open any world in the current scene

The gallery reads directly from `workspace/skills/` — no database needed. Each world card shows:

- Name, entity count, creation date
- Generation source (interactive, headless, experiment, mcp)
- Style tags and description
- Thumbnail (when available)

## Architecture

```
                    ┌──────────────────────────────────────┐
                    │           User Interfaces             │
                    │                                       │
                    │  HEARTBEAT.md    CLI flags    MCP     │
                    │  (queue tasks)   (--headless) (tools) │
                    └─────┬──────────────┬───────────┬─────┘
                          │              │           │
                          ▼              ▼           ▼
                    ┌──────────────────────────────────────┐
                    │        Experiment Dispatcher           │
                    │                                       │
                    │  Parses tasks   GPU lock   Tracking   │
                    │  from any       (flock)    (JSONL)    │
                    │  source                               │
                    └──────────────────┬───────────────────┘
                                       │
                                       ▼
                    ┌──────────────────────────────────────┐
                    │       Headless Bevy Runtime            │
                    │                                       │
                    │  Same GenPlugin as interactive mode    │
                    │  No window (primary_window: None)      │
                    │  Agent with gen tools + memory tools   │
                    │  Timeout watchdog (default 5 min)      │
                    └──────────────────┬───────────────────┘
                                       │
                                       ▼
                    ┌──────────────────────────────────────┐
                    │          Output Layer                  │
                    │                                       │
                    │  workspace/skills/<name>/              │
                    │    ├── world.ron    (scene manifest)   │
                    │    ├── SKILL.md     (metadata)         │
                    │    ├── history.jsonl (tool call log)   │
                    │    └── screenshots/ (thumbnails)       │
                    │                                       │
                    │  MEMORY.md  (experiment journal)       │
                    │  memory/    (daily structured logs)    │
                    └──────────────────────────────────────┘
```
