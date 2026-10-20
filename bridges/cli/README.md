# localgpt-bridge-cli

An interactive command line for a running [LocalGPT](https://github.com/localgpt-app/localgpt) daemon. It
connects over the daemon's bridge socket and gives the same chat as
`localgpt chat`, while every agent operation runs inside the daemon.

```bash
cargo install localgpt-bridge-cli
localgpt daemon start              # the daemon, from the `localgpt` crate
localgpt-bridge-cli                # --model to override the daemon's, --session to resume one
```

The bridge is a Unix domain socket that only accepts the same user, and a
bridge session gets no dangerous tools (no shell, no file writes): the
daemon decides which tools it has.

License: Apache-2.0.
