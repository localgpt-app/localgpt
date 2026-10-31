#!/usr/bin/env python3
"""macOS only: each process's share of the GPU's time, every 2 s, while the
runs go, so a run's frames can be read beside who else used the GPU — the
window server compositing every display, a browser, the web view's own GPU
process. Reads each GPU client's `accumulatedGPUTime` (nanoseconds) from
ioreg, which needs no sudo; macOS's own "Device Utilization %" reads 100
whenever the GPU never idles, so it says nothing about headroom.

usage: gpu.py <out-dir> [seconds]     (until killed, or for that long)
writes <out-dir>/gpu.csv: unix_s,process,percent — analyze.py reads it.
"""
import os
import re
import subprocess
import sys
import time

out = os.path.join(sys.argv[1], "gpu.csv")
until = time.time() + float(sys.argv[2]) if len(sys.argv) > 2 else float("inf")


def sample():
    text = subprocess.run(
        ["ioreg", "-l", "-w0", "-r", "-c", "IOGPUDeviceUserClient"], capture_output=True, text=True
    ).stdout
    per = {}
    for block in text.split("+-o ")[1:]:
        who = re.search(r'"IOUserClientCreator" = "pid \d+, ([^"]+)"', block)
        ns = re.search(r'"accumulatedGPUTime"=(\d+)', block)
        if who and ns:
            per[who.group(1)] = per.get(who.group(1), 0) + int(ns.group(1))
    return per, time.time()


os.makedirs(os.path.dirname(out) or ".", exist_ok=True)
with open(out, "a") as f:
    if f.tell() == 0:
        f.write("unix_s,process,percent\n")
    before, t0 = sample()
    while time.time() < until:
        time.sleep(2)
        now, t1 = sample()
        for name, ns in now.items():
            share = max(0, ns - before.get(name, ns)) / 1e9 / (t1 - t0) * 100
            if share >= 0.5:
                f.write(f"{t1:.0f},{name.replace(',', ' ')},{share:.1f}\n")
        f.flush()
        before, t0 = now, t1
