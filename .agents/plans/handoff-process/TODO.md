# Simple TODO handoff process

Status: active

Accepted/provenance checkpoint: `47ee8a03abecbb3aee84a49c0a590096d76ed1a7`.
Implemented at `9e01369`; later results retain their own command/commit scope.

## Completed

- [x] One seven-plan index, one checklist per plan and agent-owned scratch TODOs;
  coordinator reconciliation uses atomic replacement and expected-parent Git CAS.
- [x] Consolidated previously verified work under the user-accepted checkpoint;
  retained unfinished release, viewer, UX, embedded and restored-State work.
- [x] Removed recursive masters, archived evidence and duplicate status ledgers;
  old detail remains in Git history. Live API and authority contracts remain.
- [x] Replaced archive-byte checks with TODO consistency. All 52 retained source
  guards passed; independent review preserved their live behavior/safety checks.
- [x] Verification: reference profile 100/100 and strict verify; source contracts
  12, runner tests 50, exporter tests 15, and original/new atomic Git deletion
  fixtures passed. Scratch edits stay outside source inputs; real edits do not.
- [x] Condensed live build/validation guidance and corrected superseded CLI/SDK/
  architecture status; no product Rust source or runtime authority changed.

## Uncommitted

- [~] Produce the user-requested comprehensive onboarding handoff in `.agents/docs/HANDOFF.md`: initial design, retained historical steps/specs/decisions, current source/contracts, exact verification limits, pending work and a next-agent prompt.
- [ ] Cross-check all non-reserved live docs and retained dated plan records against the handoff coverage map; preserve originals and current TODO authority.
- [ ] Independently review completeness, relative links, Git references, safety instructions and the final clean checkpoint; reconcile completed agent scratch.

## Tests and verification

- [ ] Handoff coverage includes every retained non-reserved live doc and top-level historical plan, plus each current plan/checklist.
- [ ] Local links/source paths and in-repository checkpoint references resolve; historical unavailable references are explicitly identified.
- [ ] Independent review confirms no new release/provider/restored-State authority or unexecuted runtime pass is implied.
