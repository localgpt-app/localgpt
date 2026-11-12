# MCP Registry submission — localgpt-gen

How to list `localgpt-gen` in the
[MCP Registry](https://registry.modelcontextprotocol.io). Not done yet.
`server.json` in this directory is the registry manifest; the tools it
describes are catalogued in the repository's `AGENTS.md`, and client setup
is at <https://localgpt.app/docs/gen/mcp-server>.

## Status

| Item | Status |
|---|---|
| `server.json` manifest | Written; its version needs refreshing (below) |
| OCI image published to GHCR | Not done |
| `mcp-publisher` CLI installed | Not done |
| GitHub namespace verified | Automatic, via OAuth, at publish |

## Before submitting

The registry takes `npm`, `pypi`, `nuget`, `oci` and `mcpb` packages, not
crates.io, so a Rust binary goes up as an OCI image on GitHub Container
Registry. `packages[0].identifier` in `server.json` is already
`ghcr.io/localgpt-app/localgpt-gen`. (Native `cargo install` support would
need a validator for a `cargo` registry type, upstream at
<https://github.com/modelcontextprotocol/registry>.)

The packaging files were written at 0.3.5 and have not been built since.
Bring them up to date first:

1. **`server.json`**: `version` and `packages[0].version` say `0.3.5`; set
   both to the release being published.
2. **`Dockerfile.gen`** (repository root): it copies a `bridges/` directory
   that is no longer in the tree and creates the old `workspace` directory,
   so it does not build as written. Its tags in the header comment say
   `0.3.5` too. The namespace labels the registry verifies
   (`org.opencontainers.image.source`, `io.modelcontextprotocol.name`) are
   already in it.
3. **`smithery.yaml`**: it passes `LOCALGPT_WORKSPACE`, which nothing reads;
   Gen's directories come from `LOCALGPT_DATA_DIR`.
4. **The icon**: `icons[0].src` is
   `https://localgpt.app/img/localgpt-gen-icon.png`, which is not hosted.
   Host it, or remove the optional `icons` array.

Then build and push the image, from the repository root:

```bash
docker build -t ghcr.io/localgpt-app/localgpt-gen:<version> \
             -t ghcr.io/localgpt-app/localgpt-gen:latest \
             -f Dockerfile.gen .

echo $GITHUB_TOKEN | docker login ghcr.io -u localgpt-app --password-stdin
docker push ghcr.io/localgpt-app/localgpt-gen:<version>
docker push ghcr.io/localgpt-app/localgpt-gen:latest
```

## Submitting

```bash
brew install modelcontextprotocol/tap/mcp-publisher   # or: curl -fsSL https://static.modelcontextprotocol.io/install.sh | sh

cd crates/gen
mcp-publisher validate        # fix any schema errors first
mcp-publisher login github    # as localgpt-app, or a member of that organization;
                              # the io.github.localgpt-app/* namespace is then authorized
mcp-publisher publish         # picks up server.json from the current directory
```

From GitHub Actions, use OIDC instead of the login:

```yaml
- name: Publish to MCP Registry
  uses: modelcontextprotocol/publish-action@v1
  with:
    namespace: io.github.localgpt-app
```

The entry will read: `io.github.localgpt-app/localgpt-gen`, "LocalGPT Gen",
stdio transport, installed with
`docker run --rm -i ghcr.io/localgpt-app/localgpt-gen:<version> mcp-server`.
