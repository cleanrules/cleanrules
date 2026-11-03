// The half of the mixed test built without exceptions.

#include <cleanrules.hpp>

#if CLEANRULES_EXCEPTIONS
#error "this file is built without exceptions"
#endif

namespace cr = cleanrules;

bool run_without_exceptions() {
    cr::error_info error;
    auto rules = cr::ruleset::read("RULE r IF x THEN y = 1 DO act", cr::format::text, error);
    cr::actions failing;
    failing.add("act", [](cr::action_call &call) { call.fail("refused"); }, error);
    auto decision = rules.run(failing, cr::value::map({{"x", true}}, error), error);
    return !decision && error.code() == cr::errc::action_error &&
           error.message().find("refused") != std::string_view::npos;
}
