# Release readiness

Accepted/provenance checkpoint: `9f90e3df52ef684674ee9c103f157a2f5fbc07d3`.
This carries unfinished validation/distribution work, not release authorization.
Source version 0.15.0, SQLite v33 and JSONL v1 are unchanged.

## Completed

- [x] Earlier source, runtime and feature-cut corrections are accepted at the
  checkpoint above; source-only reference and strict verification passed.
  Earlier native source/asset tests retain their original commit/platform scope.
- [x] Five isolated hosted target builds passed at `ebfe1827`; local canonical
  assembly of their exact outputs passed the 15-file audit and Linux Full R14
  fixture (11 groups). Local assembly is not hosted native-five certification.
- [x] Actual macOS packaging reproduced across local/hosted target output;
  independent audit validated every ad-hoc code-signature page. This is not
  Developer ID signing or notarization.
- [x] CI capacity changes at `576be70b` preserve the 254-command Rust plan,
  assertions and overflow checks; eleven affected helper Nix checks passed
  locally. Documentation-only `9f90e3d` preserves selected release/check inputs.

- [x] `8fab610` declares Linux-only `prlimit` for native workspace tests and
  `tmux` for the validation shell. The real export fault test passed 1/1 with
  its declared tool; the pinned development shell resolves tmux 3.7c.
- [x] `7ac957a`: Phase59 screen-fixture width follows its exact provenance row,
  using the existing Phase61 calculation. Widening the same live PTY restored
  all four checks; the corrected fixture and all six later binary fixtures passed against
  the unchanged `8fab610` optimized binary. Phase59 source guard: 74 checks passed.

- [x] `7ac957a`: `python3 the removed validation_gate helper run --profile rust` passed
  254/254 in the adapted local Linux Actions emulator, with stable source and
  original/coordinator strict verification. The emulator wrapper exited 1 at a
  stale-output export; clearing only generated output and rerunning its unchanged
  export tail passed. This records the Rust-profile result and local export.

- [x] `2f25d40`: committed-Git `nix build …#aw` and `nix flake check` passed
  all 29 native checks. The coordinator independently verified selected outputs:
  18 unchanged Cargo results reused, 11 helpers rebuilt. The actual portable-export
  `prlimit` regression passed in the unchanged workspace check.
- [x] `2f25d40`: Full uses one exact committed Git input for its four Nix steps,
  with lock updates disabled. Raw-byte/mode admission rejects uncommitted source
  before validation and after preparation. Runner tests 59/59, integrated reference
  100/100 with strict verification, Gitless-helper controls and a tiny actual
  pinned-Git Nix evaluation passed.

## Uncommitted

- [ ] Run the complete combined Full profile on a committed release candidate,
  including its current five-target aggregate. Separate Rust and native Nix
  passes do not establish that 258-step result.
- [ ] Complete exact release-asset coverage on the remaining target hosts,
  locally or through hosted jobs. Hosted assembly/native-five was blocked by
  billing; resume it only if the owner chooses that route. Reuse unchanged
  verified assets where justified; Linux emulation does not establish missing
  macOS/Windows/ARM execution.
- [ ] Before distribution, agree the breaking-CLI version change (0.16.0 is
  proposed), review version surfaces, and authorize release. Do not publish
  breaking assets as the previous 0.15.0 release.
- [ ] Decide/configure Developer ID/Authenticode signing and any notarization
  requirements. Checksums, provenance, ad-hoc signing and native tests do not
  supply those external identities.
- [ ] Live-provider/billing compatibility and independent security review stay
  unverified unless separately authorized and actually executed.
