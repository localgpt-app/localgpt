import { useEffect, useMemo, useRef, useState } from "react";
import type { EntityEdit, SceneView } from "../bridge";
import { last, listen, send } from "../bridge";
import type { FieldEdit, Mark } from "./fields";
import { Inspector, type InspectorActions, type WorldSummary } from "./Inspector";
import { type Entity, type Kind, KINDS, patched, tree, valueAt } from "./model";
import { Outliner } from "./Outliner";

// The outliner over the inspector, as the left panel's Outliner tab holds
// them, on the world's document as the app sends it. The selection is the
// window's one selection; which groups are open, what is found and what
// the eye hid are the page's own, kept by id, so an agent's edit never
// folds what was opened. A field let go of is one patch for every
// selected thing that has it — one entry in the history — and while a
// drag holds it, the canvas shows it ahead of the document.

/** What the format leaves out, as the format means it. */
const DEFAULTS: Record<string, unknown> = {
  "transform.rotation_degrees": [0, 0, 0],
  "transform.scale": [1, 1, 1],
  "transform.visible": true,
  "material.roughness": 0.5,
  "material.metallic": 0,
};

/** A field's value on an entity, the format's default where it says none. */
function read(entity: Entity, path: string): unknown {
  const here = valueAt(entity, path);
  if (here !== undefined) return here;
  const parts = path.split(".");
  const axis = Number(parts.at(-1));
  if (Number.isInteger(axis)) {
    const whole = valueAt(entity, parts.slice(0, -1).join(".")) ?? DEFAULTS[parts.slice(0, -1).join(".")];
    return Array.isArray(whole) ? whole[axis] : undefined;
  }
  return DEFAULTS[path];
}

/** The patch writing `value` at `path` on an entity, as the format folds
 *  it — a vector whole, a shape whole, a material or a light by its one
 *  field — or null where the entity has no such field. */
function patchAt(entity: Entity, path: string, value: unknown): Record<string, unknown> | null {
  const [part, key, axis] = path.split(".");
  switch (part) {
    case "name":
      return { name: value };
    case "transform": {
      if (key === "visible") return { transform: { visible: value } };
      const now = read(entity, `transform.${key}`);
      if (!Array.isArray(now)) return null;
      const next = [...now];
      next[Number(axis)] = value;
      return { transform: { [key]: next } };
    }
    case "shape": {
      const [kind, fields] = Object.entries(entity.shape ?? {})[0] ?? [];
      if (!kind || typeof fields?.[key] !== "number") return null;
      return { shape: { [kind]: { ...fields, [key]: value } } };
    }
    case "material":
    case "light":
      return entity[part] ? { [part]: { [key]: value } } : null;
  }
  return null;
}

/** The fields a selection of several can differ in. */
function comparable(primary: Entity): string[] {
  const paths = ["transform.position", "transform.rotation_degrees", "transform.scale"].flatMap((v) => [0, 1, 2].map((i) => `${v}.${i}`));
  paths.push("transform.visible", "material.color", "material.roughness", "material.metallic", "light.color", "light.intensity");
  for (const key of Object.keys(Object.values(primary.shape ?? {})[0] ?? {})) paths.push(`shape.${key}`);
  return paths;
}

const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

/** The history's line for an edit, as a gesture's reads: "move lantern". */
function describe(path: string, edits: EntityEdit[], value: unknown, was: unknown): string {
  const names = edits.map((e) => e.name).join(", ");
  const [part, key] = path.split(".");
  if (part === "transform")
    return key === "position" ? `move ${names}` : key === "rotation_degrees" ? `turn ${names}` : key === "scale" ? `scale ${names}` : `${value ? "show" : "hide"} ${names}`;
  if (part === "shape") return `resize ${names}`;
  if (key === "color") return `recolour ${names}`;
  if (key === "intensity") return `${Number(value) > Number(was) ? "brighten" : "dim"} ${names}`;
  return `${key} ${Number(value).toFixed(2)} on ${names}`;
}

/** An edit waiting for the world's word: the history's next status says. */
type Awaiting = { path: string; what: string; at: number };

export function ScenePanel({ className = "" }: { className?: string }) {
  const [view, setView] = useState<SceneView | null>(() => last("scene")?.view ?? null);
  const [selected, setSelected] = useState<number[]>(() => last("selection")?.ids ?? []);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [query, setQuery] = useState("");
  const [kinds, setKinds] = useState<Kind[]>([]);
  const [hidden, setHidden] = useState<Set<number>>(new Set());
  /** What was let go of, shown until the document answers. */
  const [held, setHeld] = useState<Map<number, Entity> | null>(null);
  const [marks, setMarks] = useState<Record<string, Mark>>({});
  const awaiting = useRef<Awaiting | null>(null);
  const queued = useRef<EntityEdit[] | null>(null);
  const frame = useRef(0);
  const list = useRef<HTMLDivElement>(null);
  const revealed = useRef<number | undefined>(undefined);

  useEffect(
    () =>
      listen((m) => {
        if (m.t === "scene") {
          setView(m.view);
          setHeld(null);
        } else if (m.t === "selection") setSelected(m.ids);
        else if (m.t === "history" && awaiting.current) {
          const wait = awaiting.current;
          if (performance.now() - wait.at > 5000) awaiting.current = null;
          else if (m.status?.startsWith("refused")) {
            awaiting.current = null;
            setHeld(null);
            setMarks({ [wait.path]: { refused: m.status } });
          } else if (m.status?.includes(wait.what)) awaiting.current = null;
        }
      }),
    [],
  );

  const scene = useMemo(() => tree(view?.entities ?? []), [view]);
  const byId = useMemo(() => new Map((view?.entities ?? []).map((e) => [e.id, e])), [view]);
  const shown = (id: number) => held?.get(id) ?? byId.get(id);
  const chosen = selected.map(shown).filter((e): e is Entity => e !== undefined);
  const primary = chosen[0] ?? null;

  // A new primary comes into view, once: its groups open, its row shown.
  // A group folded by hand afterwards stays folded through any edit.
  const primaryId = selected[0];
  useEffect(() => {
    if (primaryId === undefined || revealed.current === primaryId || !scene.nodes.has(primaryId)) return;
    revealed.current = primaryId;
    const up: number[] = [];
    for (let at = scene.nodes.get(primaryId)?.parent; at != null; at = scene.nodes.get(at)?.parent) up.push(at);
    setExpanded((open) => (up.every((p) => open.has(p)) ? open : new Set([...open, ...up])));
    requestAnimationFrame(() => list.current?.querySelector(`[data-id="${primaryId}"]`)?.scrollIntoView({ block: "nearest" }));
  }, [primaryId, scene]);

  const select = (ids: number[]) => {
    setSelected(ids);
    setMarks({});
    send({ t: "select", ids });
  };

  /** Each selected thing's patch for a field set to `value` — the same
   *  value for all, or, for a drag, the shown one's value moved by as much
   *  as the drag moved it. */
  const editsFor = (path: string, value: unknown, absolute: boolean): EntityEdit[] => {
    if (!primary) return [];
    const by = typeof value === "number" && !absolute ? value - Number(read(primary, path)) : 0;
    return chosen.flatMap((entity) => {
      const own = entity === primary || absolute || typeof value !== "number" ? value : Number(read(entity, path)) + by;
      const patch = patchAt(entity, path, typeof own === "number" ? Math.round(own * 1e4) / 1e4 : own);
      return patch ? [{ id: entity.id, name: entity.name, patch }] : [];
    });
  };

  const preview = (edits: EntityEdit[]) => {
    queued.current = edits;
    if (frame.current) return;
    frame.current = requestAnimationFrame(() => {
      frame.current = 0;
      const edits = queued.current;
      queued.current = null;
      if (edits?.length) send({ t: "entity.preview", edits });
    });
  };
  const unqueue = () => {
    cancelAnimationFrame(frame.current);
    frame.current = 0;
    queued.current = null;
  };

  const commit = (path: string, edits: EntityEdit[], what: string) => {
    unqueue();
    if (!edits.length) return;
    // Waiting before it goes: the answer may come back in the same breath.
    awaiting.current = { path, what, at: performance.now() };
    setMarks({});
    setHeld(new Map(edits.map((e) => [e.id, patched(shown(e.id)!, e.patch)])));
    send({ t: "entity.patch", edits, what });
  };

  const field = <T,>(path: string): FieldEdit<T> | undefined => {
    if (!primary || !view?.editable) return undefined;
    return {
      from: read(primary, path) as T,
      held: (value, absolute) => {
        preview(editsFor(path, value, absolute));
        setMarks({ [path]: { scrub: "one entry when let go" } });
      },
      set: (value, absolute) => {
        const edits = editsFor(path, value, absolute);
        commit(path, edits, describe(path, edits, value, read(primary, path)));
      },
      // Back to what the document holds.
      cancel: () => {
        unqueue();
        setMarks({});
        send({
          t: "entity.preview",
          edits: chosen.flatMap((e) => {
            const written = byId.get(e.id) ?? e;
            const patch = patchAt(written, path, read(written, path));
            return patch ? [{ id: e.id, name: e.name, patch }] : [];
          }),
        });
      },
    };
  };

  const actions: InspectorActions | undefined = primary
    ? {
        field,
        toggle: (path) => {
          const value = !read(primary, path);
          const edits = editsFor(path, value, true);
          commit(path, edits, describe(path, edits, value, !value));
        },
        rename: (name) => commit("name", [{ id: primary.id, name: primary.name, patch: { name } }], `rename ${primary.name} to ${name}`),
        focus: () => send({ t: "focus", id: primary.id }),
        look: primary["ext-cinematography"]?.shot ? () => send({ t: "shot.look", id: primary.id }) : undefined,
        parent: primary.parent != null && byId.has(primary.parent) ? () => select([primary.parent!]) : undefined,
      }
    : undefined;

  const world: WorldSummary | undefined = view
    ? {
        name: view.world.name,
        counts: KINDS.flatMap((kind) => {
          const n = [...scene.nodes.values()].filter((node) => node.kind === kind).length;
          return n ? [[kind, n] as [Kind, number]] : [];
        }),
        sky: view.world.sky ?? undefined,
        song: view.world.song ?? undefined,
        tours: view.world.tours,
        shots: view.entities.filter((e) => e["ext-cinematography"]?.shot).length,
      }
    : undefined;

  const mixed = primary ? comparable(primary).filter((path) => chosen.some((e) => e !== primary && !same(read(e, path), read(primary, path)))) : [];
  const parent = primary?.parent != null ? byId.get(primary.parent)?.name : undefined;

  return (
    <div className={`flex min-h-0 flex-col bg-panel ${className}`}>
      <div ref={list} className="flex min-h-0 flex-[11] flex-col [&>aside]:flex-1">
        <Outliner
          nodes={scene.nodes}
          roots={scene.roots}
          expanded={expanded}
          selected={selected}
          query={query}
          kinds={kinds}
          hidden={hidden}
          on={{
            select: (id, extend) => {
              if (!extend) return select([id]);
              select(selected.includes(id) ? selected.filter((s) => s !== id) : [...selected, id]);
            },
            fold: (id) => {
              const next = new Set(expanded);
              if (!next.delete(id)) next.add(id);
              setExpanded(next);
            },
            query: setQuery,
            kinds: setKinds,
            eye: (id) => {
              const next = new Set(hidden);
              if (!next.delete(id)) next.add(id);
              setHidden(next);
              send({ t: "entity.hide", ids: [...next] });
            },
            focus: (id) => send({ t: "focus", id }),
          }}
        />
      </div>
      <div className="h-px shrink-0 bg-line" />
      <div className="flex min-h-0 flex-[9] flex-col [&>aside]:flex-1">
        {view ? (
          <Inspector
            entity={primary}
            others={chosen.slice(1).map((e) => e.name)}
            parent={parent}
            children={primary ? (scene.nodes.get(primary.id)?.children.length ?? 0) : 0}
            editable={view.editable}
            marks={marks}
            mixed={mixed}
            world={world}
            actions={actions}
          />
        ) : (
          <p className="px-3 py-4 leading-relaxed text-muted">No world is open.</p>
        )}
      </div>
    </div>
  );
}
