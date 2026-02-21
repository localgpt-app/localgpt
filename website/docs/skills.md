---
sidebar_position: 11
---

# Skills System

Skills are specialized instruction files (`SKILL.md`) that provide the agent with domain-specific knowledge for handling particular tasks. This system is fully compatible with OpenClaw's skills format.

## Overview

Skills allow you to:

- Define reusable instructions for common tasks (git commits, PR creation, code review)
- Gate functionality on required tools (e.g., require `gh` CLI for GitHub skills)
- Expose slash commands for direct invocation (`/commit`, `/pr`)
- Share skills across workspaces or customize per-project

## Skill Sources

Skills are loaded from two locations (in priority order):

| Location | Priority | Purpose |
|----------|----------|---------|
| `~/.local/share/localgpt/workspace/skills/` | Highest | Workspace-specific skills |
| `~/.local/share/localgpt/skills/` | Lower | User-level skills shared across workspaces |

When skills have the same name, workspace skills take precedence.

## SKILL.md Format

Each skill is a directory containing a `SKILL.md` file:

```
skills/
├── commit/
│   └── SKILL.md
├── github-pr/
│   └── SKILL.md
└── review/
    └── SKILL.md
```

### Basic Structure

```yaml
---
name: commit
description: "Create conventional git commits"
user-invocable: true
---

# Commit Skill

When creating commits, follow these rules:
1. Use conventional commit format: type(scope): description
2. Keep the first line under 72 characters
3. Include a body for complex changes
...
```

### Full Frontmatter Options

```yaml
---
name: github-pr
description: "Create and manage GitHub Pull Requests"
user-invocable: true              # Expose as /github-pr command
disable-model-invocation: false   # Include in system prompt
command-dispatch: tool            # Optional: direct tool dispatch
command-tool: bash                # Tool name for dispatch
metadata:
  openclaw:
    emoji: "🐙"
    always: false                 # Skip eligibility checks
    requires:
      bins: ["gh", "git"]         # Required binaries (all must exist)
      anyBins: ["python", "python3"]  # At least one required
      env: ["GITHUB_TOKEN"]       # Required environment variables
---
```

### Frontmatter Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | string | directory name | Skill identifier |
| `description` | string | - | Short description for `/skills` list |
| `user-invocable` | boolean | `true` | Expose as slash command |
| `disable-model-invocation` | boolean | `false` | Hide from model's system prompt |
| `command-dispatch` | string | - | Enable direct dispatch (`tool`) |
| `command-tool` | string | - | Tool name for dispatch |
| `metadata.openclaw.emoji` | string | - | Display emoji in skill list |
| `metadata.openclaw.always` | boolean | `false` | Skip requirement checks |
| `metadata.openclaw.requires.bins` | string[] | - | Required binaries (all) |
| `metadata.openclaw.requires.anyBins` | string[] | - | Required binaries (any) |
| `metadata.openclaw.requires.env` | string[] | - | Required environment variables |

## Using Skills

### List Available Skills

```bash
# In chat mode
/skills
```

Output:
```
Skills: 2 ready, 1 blocked

Ready:
  /commit 📝 - Create conventional git commits [workspace]
  /github-pr 🐙 - Create GitHub PRs [managed]

Blocked:
  deploy - missing bins: kubectl, helm
```

### Invoke a Skill

```bash
# In chat mode
/commit
/github-pr create a PR for this feature branch
/review check the last 3 commits
```

When invoked, the skill's instructions are loaded and the agent follows them.

## Example Skills

### Git Commit Skill

```yaml
---
name: commit
description: "Create conventional git commits"
user-invocable: true
metadata:
  openclaw:
    emoji: "📝"
    requires:
      bins: ["git"]
---

# Commit Skill

Create commits following conventional commit format.

## Format

type(scope): description

[optional body]

## Types

- feat: New feature
- fix: Bug fix
- docs: Documentation
- style: Code style (formatting, semicolons)
- refactor: Code refactoring
- test: Tests
- chore: Build, tooling, dependencies

## Rules

1. Keep the subject line under 72 characters
2. Use imperative mood ("add" not "added")
3. Don't end with a period
4. Separate subject from body with blank line
```

### GitHub PR Skill

```yaml
---
name: github-pr
description: "Create GitHub Pull Requests"
user-invocable: true
metadata:
  openclaw:
    emoji: "🐙"
    requires:
      bins: ["gh", "git"]
---

# GitHub PR Skill

Create pull requests using the GitHub CLI.

## Workflow

1. Ensure all changes are committed
2. Push branch to remote
3. Create PR with descriptive title and body
4. Request reviewers if specified

## Commands

- gh pr create --title "..." --body "..."
- gh pr view
- gh pr list
```

## Requirement Gating

Skills can specify requirements that must be met before they're available:

### Binary Requirements

```yaml
requires:
  bins: ["docker", "kubectl"]     # ALL must be present
  anyBins: ["python", "python3"]  # At least ONE must be present
```

If requirements aren't met:
- Skill shows as blocked in `/skills`
- Attempting to invoke shows a clear error message

### Environment Variables

```yaml
requires:
  env: ["AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY"]
```

## Model Invocation Control

By default, skills are included in the model's system prompt so it can suggest using them. You can control this:

```yaml
# Skill is available via /command but not mentioned to model
user-invocable: true
disable-model-invocation: true
```

Use cases:
- Skills with sensitive instructions you don't want in the prompt
- Reducing prompt size when you have many skills
- Skills that should only be used when explicitly requested

## Tips

- Keep skill instructions concise and actionable
- Use markdown formatting for readability
- Test skills with different inputs
- Use requirement gating to prevent errors from missing tools
- Organize related skills in the same workspace

## OpenClaw Compatibility

LocalGPT's skills system is fully compatible with OpenClaw. You can copy skills directly:

```bash
cp -r ~/.openclaw/skills/* ~/.local/share/localgpt/skills/
cp -r ~/.openclaw/workspace/skills/* ~/.local/share/localgpt/workspace/skills/
```

All frontmatter fields and features are supported.
