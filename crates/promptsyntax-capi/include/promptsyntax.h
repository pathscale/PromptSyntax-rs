#ifndef PROMPTSYNTAX_H
#define PROMPTSYNTAX_H

#include <stddef.h>
#include <stdint.h>

#if defined(__cplusplus)
extern "C" {
#endif

#if defined(_WIN32) && defined(PROMPTSYNTAX_CAPI_BUILD)
#define PS_API __declspec(dllexport)
#elif defined(_WIN32)
#define PS_API __declspec(dllimport)
#elif defined(__GNUC__) || defined(__clang__)
#define PS_API __attribute__((visibility("default")))
#else
#define PS_API
#endif

#define PS_ABI_VERSION ((uint32_t)0x00010000)

typedef uint32_t ps_status_t;

#define PS_STATUS_OK ((ps_status_t)0)
#define PS_STATUS_NULL_POINTER ((ps_status_t)1)
#define PS_STATUS_INVALID_UTF8 ((ps_status_t)2)
#define PS_STATUS_SERIALIZATION_ERROR ((ps_status_t)3)
#define PS_STATUS_PANIC ((ps_status_t)4)

typedef struct PsParser ps_parser_t;
typedef struct PsParseResult ps_parse_result_t;

/*
 * All input strings are UTF-8 byte spans, not C strings. A null data pointer is valid
 * only when its length is zero. Handles and borrowed views are not safe for concurrent
 * mutation; callers must provide synchronization.
 */

PS_API uint32_t ps_abi_version(void);
PS_API const char *ps_status_message(ps_status_t status);

PS_API ps_status_t ps_parser_new(ps_parser_t **out_parser);
PS_API void ps_parser_free(ps_parser_t *parser);

PS_API ps_status_t ps_parser_add_entity(
    ps_parser_t *parser,
    const uint8_t *data,
    size_t length
);
PS_API ps_status_t ps_parser_add_action(
    ps_parser_t *parser,
    const uint8_t *data,
    size_t length
);
PS_API ps_status_t ps_parser_add_authoring_namespace(
    ps_parser_t *parser,
    const uint8_t *data,
    size_t length
);

PS_API ps_status_t ps_parser_parse(
    const ps_parser_t *parser,
    const uint8_t *data,
    size_t length,
    ps_parse_result_t **out_result
);

/* The borrowed JSON bytes are valid until ps_parse_result_free(result). */
PS_API ps_status_t ps_parse_result_json(
    const ps_parse_result_t *result,
    const uint8_t **out_data,
    size_t *out_length
);
PS_API void ps_parse_result_free(ps_parse_result_t *result);

#if defined(__cplusplus)
}
#endif

#endif
