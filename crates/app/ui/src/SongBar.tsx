import type { NowPlaying } from "./bridge";
import { send } from "./bridge";
import { Icon } from "./Icon";
import { timecode } from "./timecode";

/** The song the world performs, under it: title and artist, where it is,
 *  play and pause. It goes once the timeline is built: that runs across
 *  the window, and this transport becomes its toolbar's. */
export function SongBar({ song }: { song: NowPlaying }) {
  const at = song.duration > 0 ? Math.min(1, song.position / song.duration) : 0;
  return (
    <div className="flex h-11 shrink-0 items-center gap-3 border-t border-line bg-panel px-3">
      <button
        title={song.playing ? "Pause" : "Play"}
        aria-label={song.playing ? "Pause" : "Play"}
        disabled={!song.playable}
        onClick={() => send({ t: "song.toggle" })}
        className="grid size-7 shrink-0 place-items-center rounded text-muted hover:bg-raised hover:text-ink disabled:opacity-40"
      >
        <Icon name={song.playing ? "pause" : "play"} />
      </button>
      <Icon name="music" className="size-3.5 shrink-0 text-sound" />
      <span className="min-w-0 max-w-72 truncate">
        <span className="text-ink">{song.title}</span>
        {song.artist && <span className="text-muted"> — {song.artist}</span>}
      </span>
      {song.duration > 0 && (
        <>
          <span className="relative h-1 min-w-24 flex-1 rounded bg-line">
            <span className="absolute inset-y-0 left-0 rounded bg-sound/70" style={{ width: `${at * 100}%` }} />
          </span>
          <span className="shrink-0 font-mono text-[11px] text-muted tabular-nums">
            {timecode(song.position)} / {timecode(song.duration)}
          </span>
        </>
      )}
      {!song.playable && <span className="shrink-0 text-[11px] text-faint">no audio with this world</span>}
    </div>
  );
}
