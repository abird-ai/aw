# Per-caller provider example

`cargo run --locked -p aw-account-example` uses labeled fake providers. Alice, Bob and the team account each produce an invocation record in the same conversation. Bob cannot select Alice's personal account. This is an SDK mechanics example, not authentication, real inference or verified billing.

The local-executor boundary is enforced by the registry. Actual user-local transports run on the user's device; a future remote conversation service exchanges authorized requests/results, never API keys.
