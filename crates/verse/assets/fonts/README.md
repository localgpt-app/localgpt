# Fonts

LocalGPT Verse's two typefaces, both under the SIL Open Font License (safe to bundle
and redistribute):

- **Marcellus** (`Marcellus-Regular.ttf`) — serif display face for world names
  and hero titles.
- **Hanken Grotesk** (`HankenGrotesk.ttf`) — sans UI face for labels, controls,
  and tabular time. This is the **variable** font; Bevy renders its default
  instance, so the UI weight roles (regular/medium/semibold in `theme::Fonts`)
  currently share it. Static instances could be dropped in later for distinct
  weights.

License texts: `OFL-Marcellus.txt`, `OFL-HankenGrotesk.txt`.

Sources: [Marcellus](https://fonts.google.com/specimen/Marcellus) ·
[Hanken Grotesk](https://fonts.google.com/specimen/Hanken+Grotesk) (via the
`google/fonts` repo).

`theme::Fonts::load` loads these if present and falls back to Bevy's embedded
font otherwise, so the app runs without them — but with them the `·`/`—`
glyphs render and the type matches the design spec.
