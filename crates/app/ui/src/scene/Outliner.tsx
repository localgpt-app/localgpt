import { Icon, type IconName } from "../Icon";
import { type Kind, KINDS, matching, type Node } from "./model";

export const GLYPH: Record<Kind, IconName> = { shape: "cube", mesh: "mesh", light: "bulb", camera: "camera", sound: "speaker", group: "group" };
export const TONE: Record<Kind, string> = {
  shape: "text-muted",
  mesh: "text-accent",
  light: "text-head",
  camera: "text-camera",
  sound: "text-ambience",
  group: "text-faint",
};
const LABEL: Record<Kind, string> = { shape: "Shapes", mesh: "Meshes", light: "Lights", camera: "Cameras", sound: "Sound", group: "Groups" };

/** Where a held row would go if let go: before or after a row, or into it. */
export type Drop = { held: number; id: number; zone: "before" | "after" | "into"; label: string };

export type OutlinerProps = {
  nodes: Map<number, Node>;
  roots: number[];
  expanded: Set<number>;
  /** The selection, its primary first. */
  selected?: number[];
  /** What the filter holds: a name to find, kinds to keep. */
  query?: string;
  kinds?: Kind[];
  /** The newest change someone else made, by id: who, and when. */
  changed?: Map<number, string>;
  /** What an agent's waiting proposal touches, by id: whose. */
  proposed?: Map<number, string>;
  drop?: Drop | null;
  hover?: number | null;
};

/** The world's things as a tree: what holds what, what kind each is, and
 *  what moves it. Selecting here is selecting in the world. */
export function Outliner({ nodes, roots, expanded, selected = [], query = "", kinds = [], changed, proposed, drop, hover }: OutlinerProps) {
  const keep = matching(nodes, query, kinds);
  const rows: { node: Node; depth: number }[] = [];
  const walk = (id: number, depth: number) => {
    const node = nodes.get(id);
    if (!node || (keep && !keep.has(id))) return;
    rows.push({ node, depth });
    // A filter opens the way down to what it found.
    if (keep || expanded.has(id)) node.children.forEach((child) => walk(child, depth + 1));
  };
  roots.forEach((id) => walk(id, 0));
  return (
    <aside className="flex min-h-0 flex-col bg-panel">
      <div className="flex shrink-0 flex-col gap-1.5 border-b border-line px-2 py-2">
        <span className={`flex h-7 items-center gap-1.5 rounded bg-raised px-2 ${query ? "ring-1 ring-accent/60" : ""}`}>
          <Icon name="search" className="size-3.5 text-faint" />
          <span className={query ? "text-ink" : "text-faint"}>{query || "Find by name"}</span>
          {query && <Icon name="close" className="ml-auto size-3 text-muted" />}
        </span>
        <span className="flex flex-wrap gap-0.5">
          {KINDS.map((kind) => {
            const on = kinds.includes(kind);
            return (
              <span key={kind} className={`flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] ${on ? "bg-accent/15 text-accent" : "text-muted"}`}>
                <Icon name={GLYPH[kind]} className={`size-3 ${on ? "" : TONE[kind]}`} />
                {LABEL[kind]}
              </span>
            );
          })}
        </span>
      </div>
      <div className="relative min-h-0 flex-1 overflow-y-auto py-1">
        {rows.map(({ node, depth }) => (
          <Line
            key={node.id}
            node={node}
            depth={depth}
            open={!!keep || expanded.has(node.id)}
            primary={selected[0] === node.id}
            also={selected.indexOf(node.id) > 0}
            query={query}
            dimmed={!!keep && !matchesSelf(node, query, kinds)}
            changed={changed?.get(node.id)}
            proposed={proposed?.get(node.id)}
            drop={drop?.id === node.id ? drop : null}
            held={drop?.held === node.id}
            hovered={hover === node.id}
          />
        ))}
        {rows.length === 0 && <p className="px-3 py-4 leading-relaxed text-muted">Nothing in this world is called “{query}”.</p>}
      </div>
    </aside>
  );
}

function matchesSelf(node: Node, query: string, kinds: Kind[]) {
  const q = query.trim().toLowerCase();
  return (!q || node.name.toLowerCase().includes(q)) && (kinds.length === 0 || kinds.includes(node.kind));
}

/** The name, with what the filter found in it marked. */
function Named({ name, query }: { name: string; query: string }) {
  const at = query ? name.toLowerCase().indexOf(query.trim().toLowerCase()) : -1;
  if (at < 0) return <>{name}</>;
  const end = at + query.trim().length;
  return (
    <>
      {name.slice(0, at)}
      <span className="rounded-sm bg-accent/25 text-ink">{name.slice(at, end)}</span>
      {name.slice(end)}
    </>
  );
}

function Line(props: {
  node: Node;
  depth: number;
  open: boolean;
  primary: boolean;
  also: boolean;
  query: string;
  dimmed: boolean;
  changed?: string;
  proposed?: string;
  drop: Drop | null;
  held: boolean;
  hovered: boolean;
}) {
  const { node, depth, open, primary, also, query, dimmed, changed, proposed, drop, held, hovered } = props;
  const indent = 6 + depth * 14;
  const bg = primary ? "bg-accent/20" : also ? "bg-accent/10" : drop?.zone === "into" ? "bg-accent/10" : hovered ? "bg-raised" : "";
  return (
    <div
      className={`relative flex h-6 items-center gap-1.5 pr-2 ${bg} ${held ? "opacity-40" : ""} ${drop?.zone === "into" ? "ring-1 ring-accent ring-inset" : ""} ${proposed ? "outline-1 -outline-offset-1 outline-head/70 outline-dashed" : ""}`}
      style={{ paddingLeft: indent }}
    >
      {primary && <span className="absolute inset-y-0 left-0 w-0.5 bg-accent" />}
      {node.children.length > 0 ? (
        <Icon name={open ? "chevronDown" : "chevronRight"} className="size-3 shrink-0 text-faint" />
      ) : (
        <span className="w-3 shrink-0" />
      )}
      <Icon name={GLYPH[node.kind]} className={`size-3.5 shrink-0 ${TONE[node.kind]} ${node.visible ? "" : "opacity-40"}`} />
      <span className={`min-w-0 truncate ${!node.visible || dimmed ? "text-faint" : primary ? "text-ink" : "text-ink/90"}`}>
        <Named name={node.name} query={query} />
      </span>
      {node.detail && <span className="min-w-0 shrink truncate text-[10px] text-faint">{node.detail}</span>}
      <span className="ml-auto flex shrink-0 items-center gap-1">
        {proposed && <span className="rounded-sm bg-head/15 px-1 text-[9px] text-head">{proposed}</span>}
        {changed && !proposed && <span className="rounded-sm bg-head/10 px-1 text-[9px] text-head/90">{changed}</span>}
        {node.shot && <span className="rounded-sm bg-shot/15 px-1 text-[9px] text-shot">{node.shot}</span>}
        {node.behaviors > 0 && <Icon name="spin" className="size-3 text-behavior" />}
        {node.modulations > 0 && <Icon name="wave" className="size-3 text-mod" />}
        {!node.visible ? <Icon name="hidden" className="size-3 text-muted" /> : hovered && <Icon name="eye" className="size-3 text-faint" />}
      </span>
      {drop && drop.zone !== "into" && (
        <span className="absolute right-1 h-0.5 rounded bg-accent" style={{ left: indent, [drop.zone === "before" ? "top" : "bottom"]: -1 }} />
      )}
      {drop && (
        <span className="absolute top-full right-1 z-10 mt-1 rounded bg-accent px-1.5 py-0.5 text-[10px] whitespace-nowrap text-panel">{drop.label}</span>
      )}
    </div>
  );
}
