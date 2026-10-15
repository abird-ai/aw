# Independent C11 process extension — JSONL v2

The R7 aw extension interface is a **versioned process protocol**, not an in-process Rust or C ABI. This tiny standalone example proves that a C tool/hook can communicate with normal aw through explicit newline-delimited JSON, with the same tool/journal/grant engine as a Rust extension.

Build:

    cc -std=c11 -Wall -Wextra -Werror -O2 hello.c -o /tmp/aw-extension-c

Inspect (no model call or execution grant):

    aw extension --extension-process box=/tmp/aw-extension-c --json

Explicitly allow its selected tool:

    aw --profile llama --model MODEL --extension-process box=/tmp/aw-extension-c \
       --extension-allow box:hello --print "Use the C extension"

The installed source/header and the compiled executable are **not** a shared-library ABI, a native aw C extension, or a cross-platform binary compatibility promise. The example uses fixed-shape request recognition for a disposable fixture only. Do not copy that parser for production or treat arbitrary JSON as safe. Production C extensions need a bounded, fully validating JSON encoder/decoder and proper error/cancellation/overflow behavior.

External code runs with the OS user's authority unless the user deliberately runs it under a separate host-level sandbox. aw supplies no credentials through ambient environment, and an extension manifest or readOnlyHint never grants a model tool or hook.

See [accepted history](../../.agents/plans/accepted-history/TODO.md).