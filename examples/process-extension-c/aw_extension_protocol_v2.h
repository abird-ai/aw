/* aw external process-extension protocol v2: example-only C declarations.
 * This is a JSONL process protocol, not a Rust dylib ABI or a C binary ABI.
 * Production C consumers must use a real bounded JSON parser/serializer.
 * No credentials, Session store or Agent pointer is ever handed to a child.
 */
#ifndef AW_EXTENSION_PROTOCOL_V2_H
#define AW_EXTENSION_PROTOCOL_V2_H
#define AW_EXTENSION_PROTOCOL_VERSION 2u
#define AW_EXTENSION_FRAME_MAX 262144u
/* requests: {"v":2,"id":N,"method":"describe|tool.execute|prompt|tool.call|tool.result|prompt.render","params":{...}}
 * responses: {"v":2,"id":N,"ok":true,"result":{...}}
 * A response must match the exact request ID and version; a lost response
 * makes the connection unusable and does NOT authorize effect replay.
 */
#endif