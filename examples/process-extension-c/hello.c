/*
 * Independent C process-extension example / integration fixture.
 * It uses only its own stdio, not an aw binary ABI or Rust toolchain.
 * NOTE: parsing below deliberately accepts ONLY aw's fixed encoder shape;
 * production extensions MUST use a proper bounded JSON parser, validate
 * every field, and never substring-match untrusted requests.
 */
#include "aw_extension_protocol_v2.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    char frame[AW_EXTENSION_FRAME_MAX];
    while (fgets(frame, sizeof(frame), stdin) != NULL) {
        size_t len = strlen(frame);
        if (len == 0 || frame[len - 1] != '\n') return EXIT_FAILURE;
        unsigned long long id = 0;
        if (sscanf(frame, "{\"v\":2,\"id\":%llu,", &id) != 1 || id == 0) {
            return EXIT_FAILURE;
        }
        const char *result = NULL;
        if (strstr(frame, "\"method\":\"describe\"") != NULL) {
            result = "{\"protocol\":2,\"name\":\"c-hello\",\"revision\":\"v1\","
                     "\"tools\":[{\"name\":\"hello\",\"description\":\"C extension tool\","
                     "\"input_schema\":{\"type\":\"object\",\"properties\":{},"
                     "\"additionalProperties\":false}}],"
                     "\"hooks\":[\"prompt\",\"tool.call\",\"tool.result\"],"
                     "\"prompts\":[]}";
        } else if (strstr(frame, "\"method\":\"tool.execute\"") != NULL) {
            result = "{\"content\":\"extension-c-ok\",\"is_error\":false}";
        } else if (strstr(frame, "\"method\":\"prompt\"") != NULL) {
            result = strstr(frame, "DENY_BEFORE_PROMPT") == NULL ?
                "{\"allow\":true}" : "{\"allow\":false}";
        } else if (strstr(frame, "\"method\":\"tool.call\"") != NULL) {
            result = "{\"allow\":true}";
        } else if (strstr(frame, "\"method\":\"tool.result\"") != NULL) {
            result = "{\"allow\":true}";
        } else {
            return EXIT_FAILURE;
        }
        if (printf("{\"v\":2,\"id\":%llu,\"ok\":true,\"result\":%s}\n",
                   id, result) < 0 || fflush(stdout) != 0) {
            return EXIT_FAILURE;
        }
    }
    return EXIT_SUCCESS;
}