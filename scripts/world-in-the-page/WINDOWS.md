# The world in the page, measured on Windows

A handover for running on a Windows PC the measurement that
[`docs/rfcs/authoring/the-world-in-the-page.md`](../../docs/rfcs/authoring/the-world-in-the-page.md)
ran on a Mac, and for sending the numbers back. The question: should the
desktop app's world be drawn by three.js inside its web page, rather than
by Bevy in a hole the page leaves? Two things decide it: whether the page
keeps its frames with the world in it while three agents' terminals stream,
and what each renderer costs the same machine.

It takes about an hour, most of it unattended, and it needs the machine to
itself: anything else that animates (a browser, a video, a game) takes GPU
time from the runs.

> **If you are an agent running this:** follow the steps in order, change
> no script, and record every failure with its error text. If a step fails
> twice, skip to the next part and say so in the results. Never run it over
> Remote Desktop: an RDP session usually has no hardware GPU, and every
> number would be void.

## What can be measured on Windows today

| part | what | needs |
|---|---|---|
| A, headless | The page's frames in **Edge** (WebView2's engine, so the engine a Windows host would use) and **Chromium**, with the world drawn in the page (`web`) or the hole left empty (`native`), under five terminal loads. The same runs the Mac made headless. | Node, Python |
| A, in a window | The same in an Edge window on your real monitor, so the frames keep its refresh rate (60, 144, …). | Node, Python |
| B | **Bevy alone**: the app with no page, drawing the same world in its window while the song plays. Windows has no web view host yet, so this is the app's whole half here. On the Mac the same run is the control. | Rust |

What cannot be measured yet: the page over Bevy in one window, which waits
on the Windows host (the-page.md's next step).

## What you carry from the Mac

The folder `windows-handover/`:

- `world-amber-drift.zip`: the song world the app made of the starter
  track *Amber Drift* — `manifest.json` (178 entities), the 23 CC0 models it
  uses, and the track. About 110 MB. Part A serves it to the page.
- `localgpt-feat-timeline.bundle`: the branch with this harness, as a git
  bundle, in case it has not been pushed.
- This file.

Below, the folder is at `C:\wip\windows-handover`, and everything else goes
under `C:\wip`. Use PowerShell.

## 0. Once: the tools and the machine's description

```powershell
winget install --id Git.Git -e
winget install --id OpenJS.NodeJS.LTS -e       # Node 22 or newer
winget install --id Python.Python.3.12 -e
# Part B only:
winget install --id Rustlang.Rustup -e
winget install --id Microsoft.VisualStudio.2022.BuildTools -e --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

Open a new PowerShell afterwards, so the new `PATH` is read. Then describe
the machine (the results mean little without it):

```powershell
mkdir C:\wip -Force | Out-Null
& {
  Get-CimInstance Win32_OperatingSystem | Select-Object Caption, Version, BuildNumber
  Get-CimInstance Win32_Processor | Select-Object Name, NumberOfCores, NumberOfLogicalProcessors
  Get-CimInstance Win32_ComputerSystem | Select-Object @{n="RAM GB";e={[math]::Round($_.TotalPhysicalMemory/1GB)}}
  Get-CimInstance Win32_VideoController | Select-Object Name, DriverVersion, CurrentHorizontalResolution, CurrentVerticalResolution, CurrentRefreshRate
  (Get-CimInstance -Namespace root\wmi -ClassName WmiMonitorBasicDisplayParams -ErrorAction SilentlyContinue | Measure-Object).Count | ForEach-Object { "monitors: $_" }
  powercfg /getactivescheme
} | Out-File C:\wip\machine.txt
notepad C:\wip\machine.txt
```

Add by hand at the end of `machine.txt`: each monitor's resolution, refresh
rate and scale (Settings → System → Display), whether variable refresh
(G-SYNC, FreeSync) is on, whether the PC is on mains power, and what else
was left running.

Before the runs: plug in the power, set Settings → System → Power → Power
mode to **Best performance**, and close browsers, chat apps and anything
that animates.

## 1. The code

```powershell
cd C:\wip
git clone https://github.com/localgpt-app/localgpt.git
cd localgpt
# If the branch is pushed:  git fetch origin feat/timeline:feat/timeline
git fetch C:\wip\windows-handover\localgpt-feat-timeline.bundle feat/timeline:feat/timeline
git checkout feat/timeline
```

## 2. The page and the browsers

```powershell
cd C:\wip\localgpt\crates\app\ui
npm ci
npm run build                       # the page, into crates\app\ui\dist
cd C:\wip\localgpt\website-world
npm ci                              # Playwright, the version the Mac used
npx playwright install chromium     # Edge is the one Windows already has
```

## 3. The world

```powershell
Expand-Archive C:\wip\windows-handover\world-amber-drift.zip -DestinationPath C:\wip\songs\amber-drift
Test-Path C:\wip\songs\amber-drift\manifest.json            # True
(Get-ChildItem C:\wip\songs\amber-drift\assets\models).Count  # 23
```

`C:\wip\songs` must hold that one folder and nothing else: the page's
server serves the first song folder it finds there.

## 4. Part A, headless (about 20 minutes)

```powershell
cd C:\wip\localgpt
$specs = "native:idle web:idle native:agent web:agent native:build web:build native:flood web:flood native:deluge web:deluge".Split(" ")
node scripts\world-in-the-page\headless.mjs C:\wip\songs C:\wip\out-headless msedge @specs
node scripts\world-in-the-page\headless.mjs C:\wip\songs C:\wip\out-headless chromium @specs
python scripts\world-in-the-page\analyze.py C:\wip\out-headless | Out-File -Encoding utf8 C:\wip\out-headless\tables.md
```

Each run prints `<engine>-<mode>-<load>: measured` after about 50 s. Then
check the notes at the end of `tables.md`: every run's `gl` must name your
GPU, as in `ANGLE (NVIDIA, NVIDIA GeForce RTX 4070 Direct3D11 ...)`. A
`SwiftShader` there means the browser drew in software, and those runs are
void (see *When something fails*).

The loads, per terminal (three terminals):

| load | what it is | per tab |
|---|---|---|
| agent | an agent's live region redrawn ten times a second over a streamed answer | ~7 KB/s |
| build | compiler lines with a burst every 5 s | ~14 KB/s |
| flood | coloured log lines | ~0.9 MB/s |
| deluge | an overload, to find where the page gives way | ~3.5 MB/s |

## 5. Part A, in a window on your monitor (about 6 minutes)

Headless frames are paced by the browser, not by a monitor. In a window,
they keep the monitor's rate, which is what a person would see.

```powershell
cd C:\wip\localgpt
$env:HEADED = "1"
node scripts\world-in-the-page\headless.mjs C:\wip\songs C:\wip\out-headed msedge native:idle web:idle native:agent web:agent native:flood web:flood
Remove-Item Env:HEADED
$env:HZ = "144"                     # your main monitor's refresh rate
python scripts\world-in-the-page\analyze.py C:\wip\out-headed | Out-File -Encoding utf8 C:\wip\out-headed\tables.md
Remove-Item Env:HZ
```

An Edge window opens for each run. Leave it on the main monitor, in front
and uncovered, and do not touch the mouse or the keyboard until the
command returns. A run that prints `no report` was usually covered or
minimized: run that one spec again.

## 6. Part B, Bevy alone (about 30 minutes, most of it the build)

The app finds the CC0 pack in a checkout of `localgpt-world-assets` beside
`localgpt`, through Git LFS (522 MB):

```powershell
cd C:\wip
git lfs install
git clone https://github.com/localgpt-app/localgpt-world-assets.git
cd C:\wip\localgpt
cargo build -p localgpt-app         # the dev profile is optimized; 10–20 min the first time
python scripts\world-in-the-page\alone.py ..\localgpt-world-assets\music\amber-drift.mp3 C:\wip\out-alone
# Again with the shadow in one cascade, as three.js draws it (Bevy's default is four):
$env:LOCALGPT_SPIKE_CASCADES = "1"
python scripts\world-in-the-page\alone.py ..\localgpt-world-assets\music\amber-drift.mp3 C:\wip\out-alone-1cascade
Remove-Item Env:LOCALGPT_SPIKE_CASCADES
$env:HZ = "144"                     # the refresh rate of the monitor the window opened on
python scripts\world-in-the-page\analyze.py C:\wip\out-alone | Out-File -Encoding utf8 C:\wip\out-alone\tables.md
python scripts\world-in-the-page\analyze.py C:\wip\out-alone-1cascade | Out-File -Encoding utf8 C:\wip\out-alone-1cascade\tables.md
Remove-Item Env:HZ
```

Before the window opens, the app analyses the track and, where macOS
would link the pack, copies its models into the run's folder (522 MB), so
the first launch takes a minute or two. The window shows the world alone,
with no panels: there is no page on Windows yet. It plays the song muted
after 15 s, measures 30 s and closes itself; leave it in front meanwhile. Then check
`C:\wip\out-alone\alone-idle-app.log` for the line
`localgpt-app: ... (178 entities; ...)`: 178 means the same world as the
Mac's. The frame log's comment lines (`# monitor`, `# window`) name the
displays and the window's size in pixels. The second run only changes
the shadow: on the Mac, four cascades cost Bevy twice the GPU time of one.

## 7. Send back

```powershell
cd C:\wip
robocopy C:\wip C:\wip\results machine.txt /NFL /NDL /NJH /NJS | Out-Null
foreach ($d in "out-headless", "out-headed", "out-alone", "out-alone-1cascade") {
  if (Test-Path C:\wip\$d) { robocopy C:\wip\$d C:\wip\results\$d /E /XD env /NFL /NDL /NJH /NJS | Out-Null }
}
Compress-Archive -Path C:\wip\results\* -DestinationPath C:\wip\results-windows.zip -Force
```

Send `results-windows.zip` (a few MB: each run's report, frame log, a
frame of the world and the tables), or paste `machine.txt` and the three
`tables.md`. Say which steps failed or were skipped, and why.

## When something fails

- **`python` opens the Microsoft Store.** Turn off the `python.exe` and
  `python3.exe` aliases (Settings → Apps → Advanced app settings → App
  execution aliases), or use `$env:PYTHON = "py"` for the scripts.
- **`gl` says SwiftShader.** Run at the machine, not over Remote Desktop;
  update the GPU driver; on a laptop with two GPUs, set Edge to the
  discrete one (Settings → System → Display → Graphics).
- **`msedge` is not found.** `npx playwright install msedge` (as
  administrator), or run Part A with `chromium` alone.
- **Port 8766 is taken.** `$env:PORT = "8767"`.
- **`cargo build` fails in `aws-lc-sys`.** `winget install Kitware.CMake
  NASM.NASM`, open a new PowerShell, build again. **In `ort`**: it downloads
  ONNX Runtime while building; check the network or proxy and retry.
- **`alone.py` says the pack is missing, or the log shows fewer than 178
  entities.** Run it from `C:\wip\localgpt` with the assets checkout at
  `C:\wip\localgpt-world-assets`, and `git lfs pull` there; or set
  `$env:LOCALGPT_WORLD_ASSETS` to a folder that holds the pack's `models\`.

## What the Mac measured, to compare

An M2 Max (30 GPU cores) on macOS 26.6. The full tables, and what they
mean, are in the RFC.

- **Headless, the page** (WebKit and Chromium, 1440 × 860 at 2×): 60 of
  60 fps with the world in it under the agent, build and flood loads, in
  both engines. A world frame cost the page's thread 0.3–0.7 ms and the
  GPU 6–15 ms. At ~10 MB/s of terminal output WebKit's frames went late,
  with or without the world; Chromium held.
- **Headless, displays awake**: the same page with the world held 59.5 of
  60, and the world frame's GPU wait grew to 21 ms: the window server
  was compositing three displays.
- **In the app** (window 1440 × 860 at 2×, the world in a 960 × 626 hole;
  a 60 Hz 4K main display, a 120 Hz built-in one): with Bevy drawing the
  world, Bevy and the page both ran at 30–32 fps under every load, idle
  included, and at 120 Hz too. With the page drawing it (Bevy's camera
  off), the page ran at 45–51 fps. Bevy alone, with no page, drew at
  29 fps, so on this Mac Bevy's own cost sets its rate; per process, Bevy
  used 13–16 ms of GPU a frame and the web view 4–5.
- **The shadow is most of it**: Bevy's world frame took 14 ms of GPU with
  its default four cascades and 7 ms with one (measured with the screen
  locked, where both ran at a paced 60).
- **Bevy alone** is what `alone.py` measures: compare your `alone-idle`
  rows with the Mac's 29 fps, and your Edge rows with the Mac's WebKit and
  Chromium ones.
