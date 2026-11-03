// One program, two kinds of translation unit: this one with exceptions, and
// mixed_noexcept.cpp without. Each kind gets its own inline namespace, so
// neither picks up the other's inline functions (the action trampoline, the
// end of a call), whichever the linker keeps.

#include <cleanrules.hpp>

#include <stdexcept>
#include <string>

#include "check.hpp"

#if !CLEANRULES_EXCEPTIONS
#error "this file is built with exceptions"
#endif

namespace cr = cleanrules;

// In mixed_noexcept.cpp: a run whose action fails, without exceptions.
bool run_without_exceptions();

int main() {
    auto rules = cr::ruleset::read("RULE r IF x THEN y = 1 DO act", cr::format::text);
    cr::actions throwing;
    throwing.add("act", [](cr::action_call &) { throw std::logic_error("thrown"); });
    bool nested = false;
    try {
        (void)rules.run(throwing, cr::value::map({{"x", true}}));
    } catch (const cr::action_error &e) {
        try {
            std::rethrow_if_nested(e);
        } catch (const std::logic_error &inner) {
            nested = std::string(inner.what()) == "thrown";
        }
    }
    CHECK(nested);
    CHECK(run_without_exceptions());
    return check::report("mixed");
}
