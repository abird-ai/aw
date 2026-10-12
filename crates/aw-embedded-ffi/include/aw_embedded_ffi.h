/* aw E5 / v1: independent C ABI for the shared no_std+alloc embedded Agent.
 * All inputs are (ptr,len), no C strings or Rust types across the boundary.
 * Config fields and source grants are copied on create. Host context must
 * outlive handle. Callback input/output slices are valid ONLY during call.
 * Callback output length MUST fit capacity. Host journal callback returning
 * OK means fully committed/flush durable, NOT queued. No implicit retries.
 * Every handle is single-owner; concurrent/reentrant calls return BUSY.
 * Caller must only pass live valid pointers; stale/corrupt handle pointers
 * cannot be safely distinguished. Compile staticlib with panic=abort;
 * panic never unwinds across C. OOM may abort; embedder provisions memory.
 * This ABI is NOT bare-metal firmware nor a libc-free final C executable.
 */
#ifndef AW_EMBEDDED_FFI_H
#define AW_EMBEDDED_FFI_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif

#define AW_E5_ABI_VERSION 1u
#define AW_E5_MAX_TOOLS 16u
#define AW_E5_MAX_CALLBACK_JSON 1572864u
#define AW_E5_MAX_RESULT_JSON 786432u

enum {
    AW_E5_OK=0u, AW_E5_INVALID=1u, AW_E5_ABI_MISMATCH=2u,
    AW_E5_DENIED=3u, AW_E5_UNAVAILABLE=4u, AW_E5_TOO_LARGE=5u,
    AW_E5_WRONG_PHASE=6u, AW_E5_FAILED=7u, AW_E5_BLOCKED=8u,
    AW_E5_BUFFER_TOO_SMALL=9u, AW_E5_BUSY=10u,
    AW_E5_RECONCILIATION_REQUIRED=11u
};
enum {
    AW_E5_CREATED=1u, AW_E5_NEEDS_MODEL=2u,
    AW_E5_NEEDS_TOOL=3u, AW_E5_COMPLETED=4u,
    AW_E5_INCOMPLETE=5u, AW_E5_BLOCKED_STATUS=6u
};
enum {
    AW_E5_SOURCE_HOST=1u, AW_E5_SOURCE_MCP=2u,
    AW_E5_SOURCE_EXTENSION=3u
};
enum {
    AW_E5_EFFECT_READ=1u, AW_E5_EFFECT_WRITE=2u,
    AW_E5_EFFECT_PROCESS=3u, AW_E5_EFFECT_UNKNOWN=4u
};
enum {
    AW_E5_JOURNAL_TURN_STARTED=1u,
    AW_E5_JOURNAL_MODEL_STARTED=2u,
    AW_E5_JOURNAL_MODEL_SETTLED=3u,
    AW_E5_JOURNAL_ASSISTANT=4u,
    AW_E5_JOURNAL_TOOL_INTENT=5u,
    AW_E5_JOURNAL_TOOL_RECEIPT=6u,
    AW_E5_JOURNAL_TURN_FINISHED=7u
};
enum {
    AW_E5_PORT_OK=0u, AW_E5_PORT_UNAVAILABLE=1u,
    AW_E5_PORT_DENIED=2u, AW_E5_PORT_FAILED=3u
};
enum {
    AW_E5_EVENT_MODEL_STARTED=1u, AW_E5_EVENT_TEXT_COMPLETED=2u,
    AW_E5_EVENT_TEXT_INCOMPLETE=3u, AW_E5_EVENT_BATCH_PROPOSED=4u,
    AW_E5_EVENT_BATCH_ADMITTED=5u, AW_E5_EVENT_TOOL_INTENT=6u,
    AW_E5_EVENT_INTENT_RECORDED=7u, AW_E5_EVENT_EFFECT_STARTED=8u,
    AW_E5_EVENT_EFFECT_SETTLED=9u, AW_E5_EVENT_RECEIPT_RECORDED=10u,
    AW_E5_EVENT_BLOCKED=11u,
    AW_E5_EVENT_READ_WAVE_PROPOSED=12u,
    AW_E5_EVENT_READ_WAVE_INTENT=13u,
    AW_E5_EVENT_READ_WAVE_INTENT_RECORDED=14u,
    AW_E5_EVENT_READ_WAVE_STARTED=15u,
    AW_E5_EVENT_READ_WAVE_SETTLED=16u,
    AW_E5_EVENT_READ_WAVE_RECEIPT_RECORDED=17u,
    AW_E5_EVENT_BASH_WAVE_PROPOSED=18u,
    AW_E5_EVENT_BASH_WAVE_INTENT=19u,
    AW_E5_EVENT_BASH_WAVE_INTENT_RECORDED=20u,
    AW_E5_EVENT_BASH_WAVE_STARTED=21u,
    AW_E5_EVENT_BASH_WAVE_SETTLED=22u,
    AW_E5_EVENT_BASH_WAVE_RECEIPT_RECORDED=23u
};
typedef struct AwE5Engine AwE5Engine; /* opaque, never dereference in C */

/* Tool arrays have exact V1 stride: struct_size MUST equal sizeof(AwE5ToolV1).
 * A larger element is not prefix-compatible inside a tightly packed array;
 * a future ABI must negotiate an explicit stride before extending it. */
typedef struct {
    uint32_t struct_size;
    uint32_t source_kind;
    uint32_t effect;
    uint32_t reserved;
    const uint8_t *name;
    size_t name_len;
    const uint8_t *source_id;
    size_t source_id_len;
    uint64_t source_epoch;
} AwE5ToolV1;

typedef struct {
    uint32_t struct_size;
    uint32_t abi_version;
    uint64_t turn_id;
    const uint8_t *actor; size_t actor_len;
    const uint8_t *site; size_t site_len;
    const uint8_t *account; size_t account_len;
    const uint8_t *model; size_t model_len;
    uint32_t max_rounds;
    uint32_t max_calls;
    uint64_t deadline_ticks;
    const AwE5ToolV1 *tools;
    size_t tools_count;
} AwE5ConfigV1;

typedef uint32_t (*AwE5ClockFn)(void *ctx, uint64_t *ticks);
typedef uint32_t (*AwE5EntropyFn)(void *ctx, uint8_t *out, size_t len);
typedef uint32_t (*AwE5JournalFn)(void *ctx,uint32_t kind,
                                  const uint8_t *json,size_t len);
/* Both callback pointers and output buffer are borrowed for this call only.
 * Output buffer is of capacity cap; callback sets 0<=written<=cap. */
typedef uint32_t (*AwE5ExchangeFn)(void *ctx,
                                   const uint8_t *request,size_t len,
                                   uint8_t *out,size_t cap,size_t *written);
typedef uint32_t (*AwE5GateFn)(void *ctx,const uint8_t *request,size_t len);

typedef struct {
    uint32_t struct_size;
    uint32_t abi_version;
    void *context;
    AwE5ClockFn clock;
    AwE5EntropyFn entropy;
    AwE5JournalFn journal;
    AwE5ExchangeFn model;
    AwE5GateFn tool_authorize;
    AwE5GateFn tool_validate;
    AwE5ExchangeFn tool_execute;
} AwE5CallbacksV1;

/* needs_reconciliation covers uncertain tool effects AND a paid model
 * attempt whose journal ModelStarted was committed but ModelSettled was not.
 * A false result cannot authorize automatic retry of any other effect. */
typedef struct {
    uint32_t struct_size;
    uint32_t abi_version;
    uint32_t status;
    uint32_t needs_reconciliation;
    uint32_t reserved;
    uint32_t trace_length;
    size_t answer_bytes;
    uint64_t nonce;
} AwE5StateV1;
typedef struct {
    uint32_t kind;
    uint32_t ordinal;
} AwE5EventV1;
typedef struct {
    uint32_t struct_size;
    uint32_t abi_version;
    uint32_t config_size;
    uint32_t callbacks_size;
    uint32_t tool_size;
    uint32_t state_size;
    uint32_t event_size;
    uint32_t max_tools;
    uint32_t max_callback_json;
} AwE5LayoutV1;

/* No host callbacks, network, journal or effects occur during creation. */
uint32_t aw_e5_abi_version(void);
uint32_t aw_e5_layout_v1(AwE5LayoutV1 *out);
uint32_t aw_e5_create_v1(const AwE5ConfigV1 *cfg,
                         const AwE5CallbacksV1 *callbacks,
                         AwE5Engine **out);

/* Each call drives ONE explicit transition; result is state value above. */
uint32_t aw_e5_start(AwE5Engine *engine,const uint8_t *prompt,
                     size_t len,uint32_t *progress);
uint32_t aw_e5_model_step(AwE5Engine *engine,uint32_t *progress);
uint32_t aw_e5_tool_step(AwE5Engine *engine,uint32_t *progress);
uint32_t aw_e5_cancel(AwE5Engine *engine);
uint32_t aw_e5_state(AwE5Engine *engine,AwE5StateV1 *out);

/* Output is never NUL-terminated. Caller owns it. On BUFFER_TOO_SMALL,
 * written receives exact required bytes/event-count. No retained slices. */
uint32_t aw_e5_answer_copy(AwE5Engine *engine,uint8_t *out,
                           size_t cap,size_t *written);
uint32_t aw_e5_trace_copy(AwE5Engine *engine,AwE5EventV1 *out,
                          size_t cap,size_t *written);

/* Frees and nulls caller slot; null *slot is idempotent. Uncertain effects
 * block teardown unless discard_unreconciled=1 was explicitly chosen after
 * independent host inspection; discard never means effect succeeded. */
uint32_t aw_e5_destroy(AwE5Engine **slot,uint32_t discard_unreconciled);
#ifdef __cplusplus
}
#endif
#endif
