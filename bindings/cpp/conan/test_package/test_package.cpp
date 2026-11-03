// A program that uses the cleanrules package: read rules, evaluate.

#include <cleanrules.hpp>

#include <iostream>

int main() {
    namespace cr = cleanrules;
    auto rules = cr::ruleset::read("RULE vip IF years > 5 THEN discount = 0.15", cr::format::text);
    auto decision = rules.evaluate(cr::value::map({{"years", 7}}));
    std::cout << "cleanrules " << cr::version() << ": "
              << decision.rule_name(decision.selected().at(0)) << "\n";
    return 0;
}
