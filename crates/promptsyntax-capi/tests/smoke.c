#include "promptsyntax.h"

#include <assert.h>
#include <stdlib.h>
#include <string.h>

int main(void) {
    ps_parser_t *parser = NULL;
    assert(ps_abi_version() == PS_ABI_VERSION);
    assert(ps_parser_new(&parser) == PS_STATUS_OK);

    const uint8_t entity[] = "opus";
    assert(ps_parser_add_entity(parser, entity, sizeof(entity) - 1) == PS_STATUS_OK);

    const uint8_t source[] = "@opus Summarize @file:q3.md";
    ps_parse_result_t *result = NULL;
    assert(
        ps_parser_parse(parser, source, sizeof(source) - 1, &result) == PS_STATUS_OK
    );

    const uint8_t *json = NULL;
    size_t json_length = 0;
    assert(ps_parse_result_json(result, &json, &json_length) == PS_STATUS_OK);
    assert(json != NULL);
    assert(json_length > 0);

    char *terminated = malloc(json_length + 1);
    assert(terminated != NULL);
    memcpy(terminated, json, json_length);
    terminated[json_length] = '\0';
    assert(strstr(terminated, "\"schema\":\"org.promptsyntax.parse-result/0.1\"") != NULL);
    assert(strstr(terminated, "\"data_plane\":\" Summarize \"") != NULL);

    free(terminated);
    ps_parse_result_free(result);
    ps_parser_free(parser);
    return 0;
}
