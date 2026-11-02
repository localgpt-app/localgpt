---
genre: deck
---

# Markdown, as a place

A talk you can walk through — LocalGPT MD in six slides.

---

## One file, one world

Open any Markdown file and each section becomes a place on a path.

- `##` headings become places
- save the file and the world rebuilds
- arrow keys walk the talk

---

## The draft renders instantly

A rule-based draft lays out the world the moment the file opens — no model,
no waiting. Every place is deterministic: same text, same world.

---

## Then an agent reads your slides

Put the coding agent you already use on the file: what it authors for a
slide goes into the sidecar beside the deck, and the place changes with it.
No model runs in here.

```bash
localgpt world mcp          # the deck's world, as tools for the agent
```

---

## Nothing is generated twice

Each place is cached by its content hash in a sidecar next to the deck.
Edit one slide and only that slide regenerates.

- deck.md — the talk
- deck.world.json — the cache

---

## Thank you

github.com/localgpt-app/localgpt-md
