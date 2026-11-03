// The C++ API built without exceptions: only the overloads that take an
// error_info exist, and the constructors turn what they cannot build into a
// null with the reason read_error.

#include <cleanrules.hpp>

#include <string>
#include <vector>

#include "check.hpp"

#if CLEANRULES_EXCEPTIONS
#error "this test is built without exceptions"
#endif

namespace cr = cleanrules;

int main() {
    cr::error_info error;
    auto rules = cr::ruleset::read("RULE vip IF customer.years > 5 THEN discount = 0.15 DO notify",
                                   cr::format::text, error);
    CHECK(rules);
    CHECK(!error);

    auto bad = cr::ruleset::read("RULE r IF THEN", cr::format::text, error);
    CHECK(!bad);
    CHECK(error.code() == cr::errc::rule_syntax_error);

    auto input = cr::value::map({{"customer", cr::value::map({{"years", 7}}, error)}}, error);
    CHECK(!error);
    // A key that is not UTF-8: an empty value and the error.
    CHECK(!cr::value::map({{"\xff", 1}}, error));
    CHECK(error.code() == cr::errc::format_error);
    auto decision = rules.evaluate(input, error);
    CHECK(decision);
    CHECK(decision.fired(0, error));
    CHECK(decision.output(error)["discount"].as_decimal(error) == "0.15");
    CHECK(!decision.explain(cr::format::text, error).empty());

    // Actions fail by calling `fail`.
    std::vector<std::string> seen;
    cr::actions registered;
    CHECK(registered.add("notify", [&](cr::action_call &call) {
        seen.emplace_back(call.rule_name());
        call.fail("not today");
    }, error));
    auto failed = rules.run(registered, input, error);
    CHECK(!failed);
    CHECK(error.code() == cr::errc::action_error);
    // The failed run's decision, kept beyond the next use of the error_info.
    cr::decision kept = error.decision();
    CHECK(kept);
    CHECK(kept.selected() == std::vector<std::size_t>{0});
    auto later = rules.evaluate(cr::value::map({{"customer", 1}}, error), error);
    CHECK(later);
    CHECK(!error);
    CHECK(kept.fired(0, error));
    CHECK(!error);
    CHECK(seen == std::vector<std::string>{"vip"});

    // What a constructor cannot build is a null that keeps the message.
    cr::value broken(std::string("\xff"));
    CHECK(broken.is_null());
    CHECK(broken.as_null(error).reason == cr::null_reason::read_error);
    CHECK(!broken.as_null(error).detail.empty());
    CHECK(!cr::value::make(std::string_view("\xff"), error));
    CHECK(error.code() == cr::errc::format_error);

    return check::report("no_exceptions");
}
