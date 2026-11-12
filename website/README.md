# LocalGPT Website (localgpt.app)

The landing page and user documentation for [LocalGPT](https://localgpt.app), built with [Zola](https://www.getzola.org/).

The site documents the native desktop app, Gen and its live canvas, previs, MD, Verse, and `localgpt world`. It describes only features in the current workspace.

## Development

```bash
zola serve                        # http://127.0.0.1:1111
zola check --skip-external-links # internal links
zola build                       # production site in public/
```

No Node.js dependencies are required for this site. Deployment is a separate, manual step (`cargo xtask deploy app`); do not deploy as part of a documentation check.
