# Start here

1. Read [TODO.md](TODO.md) and the selected plan's checklist.
2. Confirm the live Git checkpoint and ownership with the coordinator. Read
   [working instructions](../AGENTS.md), relevant live docs and affected source.
3. Keep parallel scratch work in `.agents/runs/<plan>-<session-agent>/TODO.md`.
   Each agent writes its own checklist; the coordinator reconciles the plan.
4. Execute relevant tests. Record concise results and known implementation
   commits in the plan, then mark its index line done when nothing remains.

The accepted historical checkpoint is
`47ee8a03abecbb3aee84a49c0a590096d76ed1a7`. Use Git history for older detail;
there is no mandatory master reread, generated handoff or evidence archive.
Unchecked work remains pending regardless of that acceptance.

The checklist itself is the record: tick an item only after running its named
command on the commit you are marking. The only shared gate is the product
build and test:

```sh
bash scripts/bootstrap.sh
cargo fmt --all -- --check
cargo test --workspace --locked
node --test tests/web_model.test.mjs tests/web_workspace.test.mjs \
  tests/web_context_flow.test.mjs tests/web_multiplayer.test.mjs \
  tests/web_agent_parity.test.mjs tests/web_rich_markdown.test.mjs
```

Record the run and its result on the plan TODO line. A source check is not a
compile result, and a local run is not a native/live-provider/release
certification. See [TODO-SPEC.md](TODO-SPEC.md) and current
[release readiness](release-readiness/TODO.md) for unfinished external gates.
