---
title: "Audio models"
description: "Optional audio-analysis models a coding agent gets as tools once you install them: split a song into stems, hear its notes, label its sounds, match it to a description."
---

# Audio models

A coding agent reads text; it cannot hear. The audio models turn a recording into what an agent can read and use: a song split into drums, bass, vocals and other; the notes an instrument played; what a sound is; how well a recording fits a description. Once you install a model, an agent working on a world gets its tool beside the world's own.

They are optional and none is installed at first. Each runs on your computer's CPU, in the `localgpt` command an agent starts as its MCP server, with no GPU, no Python and no account; after the one download, nothing touches the network. They analyse audio: none of them writes or chats.

| Model | Id | What an agent gets | Download | Licence |
|---|---|---|---|---|
| Spleeter 4stems (Deezer) | `stems` | `audio_stems`: a song as four WAVs — drums, bass, vocals, other — that add back up to it, with each one's loudness per second | 157 MB | Code and weights MIT; the ONNX conversion Apache-2.0 |
| basic-pitch (Spotify) | `notes` | `audio_notes`: the notes played — pitch, start, end and velocity | 0.2 MB | Apache-2.0 |
| CED-tiny (Xiaomi) | `tags` | `audio_tags`: what a sound or a recording contains, from AudioSet's 527 labels | 6.7 MB | Weights Apache-2.0; labels CC BY-SA 4.0 (Google) |
| CLAP (LAION) | `match` | `audio_match`: how well a recording fits each of several descriptions in plain English | 207 MB | Weights Apache-2.0, code CC0; the ONNX export states no licence of its own |

## Install {#install}

In the desktop app, **Audio** in the top bar lists the models: **Install** downloads one, with its progress, and **Remove** deletes it. From a terminal:

```bash
localgpt audio list                 # each model, its download, licence, and whether it is installed
localgpt audio install stems notes  # or --all
localgpt audio remove stems
```

Every file is pinned to a fixed revision and kept only if its SHA-256 matches, so what runs is what was checked. The models live in `~/.local/share/localgpt/models/audio/`, shared by the app and the command line; `LOCALGPT_AUDIO_MODELS` moves them.

An MCP server lists its tools when it starts, so after an install, restart or reconnect your agent's `localgpt world mcp` to see the new tool.

## Where they run {#platforms}

The models run on ONNX Runtime, which `localgpt` links in. It has builds for an Apple-silicon Mac, for Windows, and for Linux with glibc 2.38 or newer (Ubuntu 24.04, Debian 13). It has none for an Intel Mac: `localgpt` there, the one inside `LocalGPT.app` included, has no `audio` command. Built from source on an Intel Mac or an older Linux, `localgpt` needs `cargo install localgpt --no-default-features`, which leaves the audio models out and everything else in.

## What an agent does with them {#agents}

In a world's folder, `localgpt world mcp` serves the installed models' tools with the world's, and writes what they make into the world's `assets/`, where its ops can use it:

- **Stems.** `audio_stems` writes `assets/stems/<song>/drums.wav` and the other three, stereo WAVs of about 10 MB a minute each. Placed as four sounds in the world — the drums on a stage, the vocals at a character — they play in phase and still add up to the song; leave the soundtrack's own file unset, or the song is heard twice. The loudness curves it returns are exactly a soundtrack's `stems`: set them, with the song's `duration`, and lights and motion bound to a stem follow that instrument as it is heard. The desktop app plays the stems; the web viewer at localgpt.world does not play an entity's sound file yet.
- **Notes.** `audio_notes` hears pitched instruments and voices, one at a time best: on a band, run it on a stem. From them an agent reads a melody's key, range and rhythm, and can tune what the world synthesizes to the song, such as a hum on its root note. They are data, not sound: no renderer plays the format's ABC notation source yet.
- **Labels.** `audio_tags` says what is in a recording ("Music", "Piano", "Rain", "Crowd"), for the whole file and every 10 seconds — a check that a sound placed in a world is the one meant.
- **Descriptions.** `audio_match` scores a recording against your words ("rain on a tin roof", "a calm piano piece"). The scores are relative to the descriptions given, so give several, contrasting ones.

Outside a world, `localgpt audio mcp` serves the tools alone, writing into the folder it was started in.

## On the command line {#cli}

```bash
localgpt audio stems song.mp3 --out stems/         # four WAVs
localgpt audio notes melody.wav                    # a note a line; --json as the tool gives it
localgpt audio tags field-recording.flac           # labels, overall and every 10 s
localgpt audio match clip.wav "rain on a tin roof" "a crowd cheering" "an engine idling"
```

They read WAV, FLAC, Ogg Vorbis, MP3, AAC/M4A, ALAC and AIFF, up to 20 minutes. On a recent laptop a three-minute song splits in about ten seconds, and the other three models take a few seconds or less.

## Licences {#licences}

Each model's licence is shown before it is installed (`localgpt audio list`, and the app's panel), its code's and its weights' apart. Two of the files are conversions by others into the ONNX format — Spleeter's (Apache-2.0) and CLAP's (no licence stated); the weights they carry are MIT and Apache-2.0. Models whose weights are licensed for research only, or whose licence is unclear, are not offered: Demucs, which [Verse](/docs/verse/) can use for its own stems when built for it, is one, because its author says its weights' licence is unclear.
