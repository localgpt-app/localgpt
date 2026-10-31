/** `m:ss`, or `m:ss.cc` with hundredths. */
export function timecode(t: number, hundredths = false): string {
  const whole = Math.floor(t + 1e-9);
  const m = Math.floor(whole / 60);
  const ss = String(whole % 60).padStart(2, "0");
  if (!hundredths) return `${m}:${ss}`;
  const cc = String(Math.min(99, Math.round((t - whole) * 100))).padStart(2, "0");
  return `${m}:${ss}.${cc}`;
}
