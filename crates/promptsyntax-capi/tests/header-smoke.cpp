#include "promptsyntax.h"

#include <cstdint>
#include <type_traits>

static_assert(PS_ABI_VERSION == UINT32_C(0x00010000));
static_assert(std::is_same_v<ps_status_t, std::uint32_t>);

int main() {
    ps_parser_t *parser = nullptr;
    ps_parse_result_t *result = nullptr;
    const std::uint8_t source[] = {'h', 'e', 'l', 'l', 'o'};

    if (ps_parser_new(&parser) != PS_STATUS_OK) {
        return 1;
    }
    const ps_status_t status =
        ps_parser_parse(parser, source, sizeof(source), &result);
    if (status == PS_STATUS_OK) {
        ps_parse_result_free(result);
    }
    ps_parser_free(parser);
    return status == PS_STATUS_OK ? 0 : 1;
}
