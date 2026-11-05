#!/usr/bin/env python3
"""What the widget must draw: box drawing, braille, powerline, emoji, CJK,
the 16 and 256 colours, true colour, bold and underline; then the terminal's
size every second (so a resize shows), for N seconds.
usage: probe.py [seconds]"""
import os, sys, time
secs = float(sys.argv[1]) if len(sys.argv) > 1 else 20
w = sys.stdout.write
w("box:   ┌─┬─┐ ├─┼─┤ └─┴─┘ ╔═╦═╗ ║ ╚═╩═╝ ░▒▓█ ▁▂▃▄▅▆▇█\n")
w("braille: ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ ⣾⣽⣻⢿⡿⣟⣯⣷   marks: ✳ ✶ ✻ ✽ ● ○ ◐ ◑ ▸ ▾ ⏺ ✓ ✗\n")
w("powerline:    ⚡   emoji: 🚀 🎉 ✅ ❌ 🤖 🧠 📦 🔥\n")
w("cjk:   你好，世界 — 中文 日本語テキスト 한국어 텍스트 | 全角：１２３ＡＢＣ\n")
w("mixed: ab你c好d — the 好 is two cells wide, the a one\n")
w("16:    " + "".join(f"\x1b[{30+i}m■\x1b[0m" for i in range(8)) + " " + "".join(f"\x1b[{90+i}m■\x1b[0m" for i in range(8)) + "\n")
w("256:   " + "".join(f"\x1b[38;5;{i}m▪" for i in range(16, 232, 4)) + "\x1b[0m\n")
w("true:  " + "".join(f"\x1b[38;2;{int(255*(1-i/48))};{int(255*i/48)};120m▪" for i in range(48)) + "\x1b[0m\n")
w("style: \x1b[1mbold\x1b[0m \x1b[3mitalic\x1b[0m \x1b[4munderline\x1b[0m \x1b[9mstrike\x1b[0m \x1b[7minverse\x1b[0m \x1b[2mdim\x1b[0m \x1b[1;31mbold red\x1b[0m\n")
w("ink:   \x1b[1m⠋\x1b[0m Thinking… (esc to interrupt)  \x1b[2m· 3s\x1b[0m\n")
sys.stdout.flush()
end = time.monotonic() + secs
while time.monotonic() < end:
    s = os.get_terminal_size()
    w(f"size:  {s.columns} cols × {s.lines} rows   {time.strftime('%H:%M:%S')}\n")
    sys.stdout.flush()
    time.sleep(1)
