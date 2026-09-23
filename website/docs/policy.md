---
sidebar_position: 16
---

# POLICY.md

## Your standing instructions to the AI — always present, near the end

`POLICY.md` is a plain-text Markdown file that lives in your workspace alongside `SOUL.md`, `MEMORY.md`, and `HEARTBEAT.md`. Whatever you write in this file will be injected near the end of every conversation turn — after all messages and tool outputs, just before a hardcoded security suffix that always occupies the final position.

This makes `POLICY.md` one of the most persistent and influential files in your workspace. It is your tenets, your ground rules, your standing orders, your guardrails, your conventions, and your reminders — all in one place, always in effect.

:::info Renamed from LocalGPT.md
This file used to be called `LocalGPT.md`. It was renamed to avoid confusion with the [LocalGPT MD app](https://md.localgpt.app/). If your workspace still has a `LocalGPT.md`, it keeps working: LocalGPT loads it whenever no `POLICY.md` exists, and the old name stays on the protected-files deny list. To migrate, simply rename the file:

```bash
mv ~/.local/share/localgpt/workspace/LocalGPT.md ~/.local/share/localgpt/workspace/POLICY.md
```
:::

## What it does

Every time the AI is about to respond, LocalGPT assembles a context window from your conversation history, tool results, memory, and system instructions. The content of `POLICY.md` is injected near the end of this context — immediately before a hardcoded security suffix that always occupies the final position.

In large language models, **position matters**. Content near the end of the context window receives stronger attention weighting. By placing your instructions in this high-attention zone, `POLICY.md` acts as a persistent anchor — a constant reminder that doesn't get buried under conversation noise, even in long sessions.

Think of it as:

- The **house rules** posted at the door — everyone sees them, every time
- A **standing brief** handed to your team at the start of every meeting
- The **principles and conventions** your AI should always keep in mind
- A **behavioral anchor** that resists drift over long conversations
- Your **standing orders** that the AI sees near the end of every turn

## What to put in it

`POLICY.md` is intentionally open-ended. It is not limited to security rules or policy — it is for anything you want the AI to consistently remember and respect. Common uses include:

**Coding conventions and standards**
```markdown
## Conventions
- Use `snake_case` for all Rust identifiers
- Every public function must have a doc comment
- Never use `unwrap()` in production code — use `?` or explicit error handling
- Prefer `thiserror` for library errors, `anyhow` for application errors
```

**Security boundaries and access rules**
```markdown
## Boundaries
- Never read or write files outside the workspace directory
- Never execute commands that require network access
- Do not modify any file in the `contracts/` directory without explicit confirmation
- Treat all user-uploaded files as untrusted input
```

**Communication and workflow preferences**
```markdown
## How I work
- Explain your reasoning before showing code
- When uncertain, say so — don't guess
- If a task will take more than 3 steps, outline the plan first
- Always suggest tests for new functionality
```

**Project-specific constraints and compliance**
```markdown
## Project rules
- This codebase must remain compatible with Rust 1.75+
- All dependencies must use MIT, Apache-2.0, or BSD licenses only
- Database migrations must be reversible
- Log all state changes at INFO level
```

**Reminders the AI tends to forget in long sessions**
```markdown
## Reminders
- The `config.toml` schema changed in v2.0 — do not use the old format
- Our CI runs on ARM64, not x86 — test accordingly
- The `legacy/` module is frozen — route new features through `core/`
```

You can combine any of these. The file is yours. Write it however makes sense for your workflow.

## How it stays trustworthy

Because `POLICY.md` directly shapes AI behavior, the agent must not be able to rewrite its own instructions. That protection comes from two system-level mechanisms, not from anything in the prompt:

1. **Protected files** — `POLICY.md` is on a hardcoded deny list. The agent's `write_file` and `edit_file` tools refuse to modify it (alongside `IDENTITY.md` and other protected paths), no matter what the model decides to do.
2. **The sandbox** — shell commands run in a kernel-enforced sandbox that cannot write outside the allowed paths, so the agent can't bypass the file tools to edit it either.

You edit `POLICY.md` yourself, in your editor of choice — it's a plain Markdown file. Changes take effect on the next turn; there is no signing step, no manifest, and nothing to re-verify.

Before injection, the file's content is also **sanitized** against common prompt-injection patterns (the same filter applied to tool output), capped at 4,096 characters, and wrapped so the model treats it as your instructions rather than as a message from itself.

If `POLICY.md` does not exist, LocalGPT simply runs without your custom instructions, using only its built-in defaults.

## Important: guidance, not guarantees

Large language models are probabilistic. `POLICY.md` provides **strong, persistent guidance** — not deterministic enforcement. The end-of-context positioning gives your instructions maximum influence, and the AI will follow them in the vast majority of interactions. But no prompt-based mechanism can guarantee 100% compliance in every edge case.

This is by design. `POLICY.md` is about shaping behavior over time, setting expectations, and keeping the AI aligned with how you work. For hard security boundaries that must never be crossed (like filesystem sandboxing or network isolation), LocalGPT enforces those at the system level, independent of any prompt.

Think of `POLICY.md` as a strong cultural norm — followed naturally and consistently, but backed by real enforcement mechanisms at the infrastructure layer where it matters most.

## How the security block is injected

Every time the AI is about to respond, LocalGPT builds a message array for the LLM API call. The security block (your policy + hardcoded suffix) is **concatenated into the last user or tool-result message** in the array — it is not sent as a separate message. This avoids consecutive same-role messages, which some LLM APIs (notably Anthropic) reject.

The security block has two independent layers:

| Layer | Source | Configurable | Position |
|-------|--------|-------------|----------|
| **User policy** | `POLICY.md` | `security.disable_policy` | Before suffix |
| **Hardcoded suffix** | Compiled into binary | `security.disable_suffix` | Always last |

The resulting text is appended to the last message with a `\n\n` separator. It is **not saved** to session logs, **not included** in compaction/summarization, and **not visible** in session transcripts — it exists only in the API call payload.

## Disabling the security block

Both layers can be independently disabled in `~/.config/localgpt/config.toml`:

```toml
[security]
# Skip loading POLICY.md workspace policy (default: false)
# The hardcoded suffix still applies.
disable_policy = false

# Skip the hardcoded security suffix (default: false)
# The user policy still applies.
disable_suffix = false
```

| `disable_policy` | `disable_suffix` | Result |
|---|---|---|
| `false` | `false` | Full security block (policy + suffix) |
| `true` | `false` | Hardcoded suffix only |
| `false` | `true` | User policy only |
| `true` | `true` | No security block injected |

:::warning
Setting both to `true` removes all end-of-context security reinforcement. The system prompt safety section still exists, but may lose effectiveness in long sessions due to the "lost in the middle" attention decay effect.
:::

## Quick reference

| | |
|---|---|
| **Location** | `~/.local/share/localgpt/workspace/POLICY.md` |
| **Format** | Plain Markdown (UTF-8) |
| **Size limit** | 4,096 characters (~1,000 tokens) |
| **Injected** | Near end of every turn (before security suffix) |
| **Sanitized** | Yes — filtered for prompt-injection patterns before injection |
| **Editable by AI** | No — protected-files deny list blocks `write_file`/`edit_file`; sandbox blocks shell edits |
| **Required** | No — LocalGPT works without it, using built-in defaults |
| **CLI** | `localgpt policy status` shows whether your policy is active |

## Getting started

Create or edit the file:

```
$ nano ~/.local/share/localgpt/workspace/POLICY.md
```

Write your instructions and save. That's it — your instructions are active from the next turn onward, for every conversation and every session, until you change them.
