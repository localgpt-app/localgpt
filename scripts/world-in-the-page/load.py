#!/usr/bin/env python3
"""What a coding agent's terminal writes, for the spike. Each of the page's
three terminals runs one.

usage: load.py <agent|build|flood|deluge> <seconds>

  agent  an Ink-style live region (14 lines) redrawn ten times a second over
         a streamed answer, committed above it every ~600 characters: what
         Claude Code or Codex writes while it works. ~7 KB/s.
  build  compiler lines, 150 a second, and a burst of 400 every 5 s: a shell
         tab running a build. ~14 KB/s.
  flood  ~0.9 MB/s of coloured log lines: `cat` of a big log, a verbose test run.
  deluge ~3.8 MB/s: not an agent, an overload to find where the page gives way.
"""
import random
import sys
import time

mode, secs = sys.argv[1], float(sys.argv[2])
out = sys.stdout.buffer
rnd = random.Random(7)
WORDS = (
    "the world folds each entry into a document and the canvas applies "
    "what was committed while agents write batches of ops to its api"
).split()


def words(n):
    return " ".join(rnd.choice(WORDS) for _ in range(n))


def write(s):
    out.write(s.encode())
    out.flush()


end = time.monotonic() + secs
if mode == "agent":
    spin, region, t0, i, answer = "⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏", 14, time.monotonic(), 0, ""
    write("\r\n" * region)
    while time.monotonic() < end:
        i += 1
        answer += " " + words(rnd.randint(2, 6))
        if len(answer) > 600:
            write(f"\x1b[{region}A\x1b[J\x1b[1m●\x1b[0m{answer}\r\n\r\n" + "\r\n" * region)
            answer = ""
        lines = [
            f"\x1b[38;5;174m{spin[i % 10]}\x1b[0m Working… "
            f"\x1b[2m({time.monotonic() - t0:.0f}s · esc to interrupt)\x1b[0m"
        ]
        lines += ["  " + answer[k : k + 76] for k in range(0, len(answer), 76)][: region - 4]
        lines += [""] * (region - 3 - len(lines))
        lines += ["\x1b[2m" + "─" * 78 + "\x1b[0m", "> \x1b[7m \x1b[0m", "\x1b[2m  ⏵⏵ accept edits on\x1b[0m"]
        write(f"\x1b[{region}A" + "".join(f"\r\x1b[2K{line}\r\n" for line in lines))
        time.sleep(0.1)
elif mode == "build":
    n, burst_at = 0, time.monotonic() + 5
    while time.monotonic() < end:
        count = 15
        if time.monotonic() >= burst_at:
            count, burst_at = 400, time.monotonic() + 5
        chunk = []
        for _ in range(count):
            n += 1
            crate = f"crate-{rnd.randrange(400)}"
            if n % 23 == 0:
                chunk.append(
                    f"\x1b[1;33mwarning\x1b[0m\x1b[1m: unused variable: `{words(1)}`\x1b[0m\r\n"
                    f"   \x1b[1;34m-->\x1b[0m crates/{crate}/src/lib.rs:{rnd.randrange(900)}:{rnd.randrange(80)}\r\n"
                )
            else:
                chunk.append(
                    f"   \x1b[1;32mCompiling\x1b[0m {crate} v0.{rnd.randrange(30)}.{rnd.randrange(9)} (/repo/crates/{crate})\r\n"
                )
        write("".join(chunk))
        time.sleep(0.1)
elif mode == "flood":
    k = 0
    while time.monotonic() < end:
        t = time.monotonic()
        write("".join(f"\x1b[36m{k + j:08d}\x1b[0m {words(14)}\r\n" for j in range(1000)))
        k += 1000
        time.sleep(max(0.0, 0.1 - (time.monotonic() - t)))  # ~100 KB, ten a second
elif mode == "deluge":
    # Not an agent: an overload, to find where the page gives way. One
    # ~95 KB block of the flood's lines, written four times ten times a
    # second: ~3.8 MB/s.
    block = "".join(f"\x1b[36m{j:08d}\x1b[0m {words(14)}\r\n" for j in range(1000)).encode()
    while time.monotonic() < end:
        t = time.monotonic()
        for _ in range(4):
            out.write(block)
        out.flush()
        time.sleep(max(0.0, 0.1 - (time.monotonic() - t)))
else:
    sys.exit(f"unknown load {mode}")
