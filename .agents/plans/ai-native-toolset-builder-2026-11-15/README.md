# Shared toolset builder

Author: Prasanna Loganathar
Co-authors: ChatGPT GPT-6
Owner: @pvl

**Current cross-repository context:** [AW–dotlink full comprehensive handoff (2026-11-17)](../../docs/AW-DOTLINK-TOOLS-FULL-COMPREHENSIVE-HANDOFF-2026-11-17.md). Includes the historical initial designs, complete implementation checkpoint index, exact Git/pin state, unresolved gates and next-agent prompt. Source and current plan TODO outrank dated handoff prose.

Establish the **reusable, configurable tool catalog and presentation contract** in canonical `aw-tools` before deciding AW/dotlink-specific variants of edit, grep, status receipts, transports or defaults.

- [Design](DESIGN.md) is normative for this new builder/factory layer.
- [TODO](TODO.md) is the implementation checkpoint.
- Existing [Job/grep convergence](../ai-native-tool-output-grep-2026-11-15/README.md) remains responsible for unfinished runtime security, ACK, wait and live-flow correctness; this plan does not claim that work is done.
- Dotlink's pending companion plan waits for a reviewed AW shared pin; no dotlink implementation or dependency-pin change here.

Toolset configuration is **not authority**. The existing Process/JobManager/Graph/Agent owner, ToolPolicy/Envelope, journal, and secured filesystem backends remain authoritative. Default model vocabulary stays flat; no second registry, runtime, scheduler, policy or generated tool aliases.
