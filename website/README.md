# LocalGPT Website (localgpt.app)

The official documentation and landing site for [LocalGPT](https://localgpt.app), built with [Zola](https://www.getzola.org/).

## Features

- **Zero Node.js dependencies**: Fast single-binary static site generator written in Rust.
- **Full documentation**: the desktop app, Gen (with the live canvas and previs), Verse, MD, and the assistant's CLI and API.
- **World Templates**: Interactive catalog of pre-configured 3D world templates.
- **Client-side search**: Integrated Elasticlunr full-text search index.

## Development

```bash
zola serve
```

Opens at `http://127.0.0.1:1111` with live reload.

## Production Build

```bash
zola build
```

Generates static assets into `public/`.
