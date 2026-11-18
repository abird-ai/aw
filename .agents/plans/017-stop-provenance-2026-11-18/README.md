# 0.17 stop provenance — completed first slice

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-5.6 Sol
Owner: @pvl

This plan now records only the completed first AW shared slice:

- physical process exit is `Code(i32) | Signal(i32) | Unknown`;
- signal identity is retained from the reaped OS status;
- cancellation/deadline/revocation remain lifecycle facts.

Implementation checkpoint:

- `796225e9b50e824606586859d7bcc88722b4919e`
  `Separate process signal identity from lifecycle`

The broader shared-first design discovered after the finalized dotlink v0.17.4
audit is owned by:

- [017-shared-runtime-convergence-2026-11-18](../017-shared-runtime-convergence-2026-11-18/README.md)

That master plan owns cancellation origin, stop cause/attempt/outcome, platform
Guard evidence, ProcessTerminal/Task propagation, lifecycle events, shared Job
receipts/refs/budgets and aw-mcp projection.

The sandbox/mount design is separately frozen in:

- [017-sandbox-mount-convergence-2026-11-18](../017-sandbox-mount-convergence-2026-11-18/README.md)

Authoritative release reference is now dotlink
`v0.17.4 @ fd1cdb6429a341a4e3d27805cae0bec288fc26df`.

No remaining implementation belongs to this leaf plan.
