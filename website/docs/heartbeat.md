---
sidebar_position: 10
---

# Heartbeat System

The heartbeat system enables LocalGPT to perform autonomous tasks on a schedule, checking `HEARTBEAT.md` for pending work.

## Overview

Heartbeat runs periodically (e.g., every 30 minutes) and:

1. Reads `HEARTBEAT.md` for pending tasks
2. Evaluates if any tasks need action
3. Executes tasks using the full agent toolset
4. Updates task status in the file

If there are no pending tasks, the heartbeat responds with `HEARTBEAT_OK` and waits for the next cycle.

## HEARTBEAT.md Format

The heartbeat file contains tasks in a simple markdown format:

```markdown
# Heartbeat Tasks

## Pending

### Check CI Status
- Priority: high
- Due: 2024-01-15 14:00
- Description: Check if the CI pipeline passed for PR #42

### Daily Summary
- Priority: low
- Recurring: daily at 18:00
- Description: Summarize today's work and update MEMORY.md

## Completed

### Review Documentation
- Completed: 2024-01-14 16:30
- Notes: Updated README with new API endpoints
```

## Configuration

Enable and configure heartbeat in `config.toml`:

```toml
[heartbeat]
# Enable heartbeat runner
enabled = true

# How often to check for tasks
interval = "30m"

# Only run during these hours (optional)
active_hours = { start = "09:00", end = "22:00" }

# Timezone for active hours (optional)
# timezone = "America/Los_Angeles"
```

### Interval Format

The interval supports various duration formats:

- `30m` - 30 minutes
- `1h` - 1 hour
- `2h30m` - 2 hours 30 minutes
- `90s` - 90 seconds (for testing)

### Active Hours

Restrict heartbeat to specific hours to avoid late-night activity:

```toml
active_hours = { start = "09:00", end = "22:00" }
```

Outside these hours, heartbeat cycles are skipped.

## Running Heartbeat

### With Daemon

The daemon automatically runs heartbeat if enabled:

```bash
localgpt daemon start
```

### Manual Execution

Run a single heartbeat cycle:

```bash
localgpt daemon heartbeat
```

This is useful for:
- Testing your heartbeat tasks
- Running from cron/scheduled tasks
- Debugging task execution

## Task Types

### One-Time Tasks

Tasks that run once and are marked complete:

```markdown
### Send Weekly Report
- Due: 2024-01-15 09:00
- Description: Generate and email the weekly status report
```

### Recurring Tasks

Tasks that repeat on a schedule:

```markdown
### Backup Database
- Recurring: daily at 03:00
- Description: Run the database backup script
```

### Conditional Tasks

Tasks that depend on external conditions:

```markdown
### Merge PR When Tests Pass
- Condition: CI status is green for PR #42
- Description: Merge the PR and delete the branch
```

## What Heartbeat Can Do

Since heartbeat uses the full agent toolset, it can:

- **Execute commands** - Run scripts, check statuses
- **Read/write files** - Update configurations, generate reports
- **Search memory** - Find relevant context
- **Make HTTP requests** - Check APIs, fetch data
- **Update memory** - Log actions and results

## Example Tasks

### Git Operations

```markdown
### Pull Latest Changes
- Recurring: hourly
- Description: Pull latest changes from main branch in ~/projects/myapp
```

### System Monitoring

```markdown
### Check Disk Space
- Recurring: daily at 08:00
- Description: Check disk usage and alert if > 80%
```

### Project Management

```markdown
### Update TODO List
- Recurring: daily at 18:00
- Description: Review completed tasks and update project TODO.md
```

## Heartbeat Cycle

Each heartbeat cycle follows this flow:

```
┌─────────────────┐
│  Timer Fires    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Check Active   │──── Outside Hours ──→ Skip
│    Hours        │
└────────┬────────┘
         │ Within Hours
         ▼
┌─────────────────┐
│  Read           │
│  HEARTBEAT.md   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Evaluate       │──── No Tasks ──→ HEARTBEAT_OK
│  Tasks          │
└────────┬────────┘
         │ Has Tasks
         ▼
┌─────────────────┐
│  Execute        │
│  Tasks          │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Update         │
│  HEARTBEAT.md   │
└────────┬────────┘
         │
         ▼
┌─────────────────┐
│  Wait for       │
│  Next Interval  │
└─────────────────┘
```

## Logging

Heartbeat actions are logged to:
- Application log (`~/.local/state/localgpt/logs/agent.log`)
- Daily memory log (if memory_append is used)

## Best Practices

1. **Start simple** - Begin with one or two tasks
2. **Test manually** - Use `daemon heartbeat` to test
3. **Set reasonable intervals** - 30m is a good default
4. **Use active hours** - Avoid unexpected late-night activity
5. **Review logs** - Check that tasks complete as expected
6. **Keep tasks focused** - One clear action per task
