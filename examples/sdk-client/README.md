# Independent local owner client

Depends on `aw-client`, not the CLI, agent loop, provider or SQLite. With no argument it opens no connection. An explicit private same-UID socket returns handshake and bounded snapshot metadata:

```sh
cargo run --locked -p aw-owner-client-example -- /absolute/private/owner.sock
```

This accesses the existing owner's configured accounts and history. It is not a user-device inference bridge, a credential transfer, or a multi-user login. Source example; actual Cargo execution must be verified on a capable host.
