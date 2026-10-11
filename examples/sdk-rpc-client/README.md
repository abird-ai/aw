# Independent aw RPC clients (Rust and Python)

This directory is a standalone **external** implementation of the versioned
stdio JSONL protocol. It does not import the aw Agent's private provider,
tool, journal, credential or Session types.

- Rust library: src/lib.rs, reusable Client<BufRead, Write> with typed frames.
- Rust process demo: src/main.rs, launches a locally selected aw executable
  without a shell. Cargo: cargo run -p aw-rpc-client-example -- --binary
  /absolute/path/aw --base-url http://127.0.0.1:8080/v1 [--full].
- Python client: python_client.py, standard library only, same arguments.

For normal CLI use, the **full** build accepts:

    aw rpc --stdio --profile llama --model MODEL --base-url URL --no-state

The JSONL-minimal feature cut accepts equivalent host settings **before** the
subcommand (the full parameter taxonomy unification is future R14):

    aw --profile llama --model MODEL --base-url URL --no-state rpc --stdio

Each stdin request is one JSON object terminated by LF:

    {"v":1,"id":"caller_1","method":"get_state","params":{}}
    {"v":1,"id":"caller_2","method":"prompt","params":{"text":"Hello"}}
    {"v":1,"id":"caller_3","method":"abort","params":{"target_id":"caller_2"}}

RPC stdout is **JSONL only**: initial hello with effective capability flags,
then ordered response / accepted / AgentEvent deltas / settled records.
Every frame has v, seq, type, id and data. The caller's ID is unique within
one live process; duplicates fail rather than trigger duplicate provider calls.

An accepted request has NOT necessarily settled its paid or tool effects.
Rust Client reports TransportUncertain on lost pipe. Client SDKs must **never automatically retry** after a broken pipe or an
unknown/partial result. On a new connection with a durable Session, specify
the EXACT --session ID and use get_state/get_messages to inspect prior effects.
"Latest" --resume is not guaranteed to select the same Session after new
Session creation. When unsure, inspect journal/effects manually.

Protocol safety bounds: v1 only, max request line 192KiB, max outbound frame
640KiB, queue 32, max distinct IDs 1024, max get_messages 12 turn projections,
bounded user/content/tool text, no raw provider-private signed context.
Method params are strictly checked; no arbitrary tool/root/account/credential
parameters can be added to a prompt. Models/accounts/tools/Session bindings
must be preconfigured through the existing trusted aw owner CLI controls.

Supported methods in these clients: prompt, prompt_template when the project
is explicitly approved, abort, get_state, get_messages, available_models,
set_model/set_effort for already-authorized stateless settings, new_session,
open_session for exact supported JSONL owner and compact when a JSONL Session
actually exists. The initial hello advertises the effective capabilities;
unsupported privileged methods, including direct trusted_bash and
extension_control, are explicitly unavailable, never success-shaped stubs.

To print a single turn's events without RPC:

    aw --output jsonl --print "Hello"

Only --diagnostics-protocol emits sanitized event-type and frame-size metadata
to **stderr**. It never records raw HTTP wire, credentials, headers, model
payloads, project data or private signed thinking context. No raw-wire capture
or live OAuth provider certification is claimed.

The user-requested comprehensive flag family categorization remains the
canonical CLI grammar (see [CLI](../../.agents/docs/CLI.md)); accepted implementation history stays in Git.
