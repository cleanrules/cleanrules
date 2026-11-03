// The rules of discounts.rules, from C++: read them, evaluate one input given
// as JSON and then built natively, print the output, the explanation and
// which rules fired, run the rules' named action (notify_sales) as a lambda,
// and report an error, both as an exception and as an error_info.
//
//     cmake -S bindings/cpp -B build/cpp && cmake --build build/cpp
//     build/cpp/discounts docs/examples/discounts.rules
//
// Or, with an installed cleanrules, from a project's CMakeLists.txt:
//
//     find_package(cleanrules CONFIG REQUIRED)
//     target_link_libraries(discounts PRIVATE cleanrules::cleanrules)

#include <cleanrules.hpp>

#include <fstream>
#include <iostream>
#include <sstream>
#include <string>

namespace cr = cleanrules;

int main(int argc, char **argv) {
    if (cr::abi_version() != cr::header_abi_version) {
        std::cerr << "this program was built for another cleanrules ABI\n";
        return 1;
    }
    if (argc != 2) {
        std::cerr << "usage: " << argv[0] << " discounts.rules\n";
        return 2;
    }
    std::ifstream file(argv[1], std::ios::binary);
    if (!file) {
        std::cerr << argv[1] << ": cannot be read\n";
        return 1;
    }
    std::ostringstream text;
    text << file.rdbuf();

    try {
        auto rules = cr::ruleset::read(text.str(), cr::format::text);

        auto decision = rules.evaluate_json(R"({"customer": {"years": 7}, "order": {"total": 1500}})");
        std::cout << "output: " << decision.output_json() << "\n\n" << decision.explain() << "\n";

        // The same input, built natively; the decision keeps a copy of it.
        auto input = cr::value::map({
            {"customer", cr::value::map({{"years", 7}})},
            {"order", cr::value::map({{"total", 1500}})},
        });
        decision = rules.evaluate(input);
        std::cout << "\n";
        for (std::size_t i = 0; i < decision.size(); ++i) {
            std::cout << rules.rule_name(i) << (decision.fired(i) ? " fired" : " did not fire")
                      << "\n";
        }
        std::cout << "discount: " << decision.output()["discount"].as_decimal() << "\n";

        // Run the rules: evaluate, then call the selected rules' actions.
        int calls = 0;
        cr::actions actions;
        actions.add("notify_sales", [&calls](cr::action_call &call) {
            ++calls;
            std::cout << "notify_sales: rule " << call.rule_name() << " was selected\n";
        });
        rules.run(actions, input);
        std::cout << "\n" << calls << " action called\n";

        // An input that is not JSON, without exceptions: the error says where.
        cr::error_info error;
        if (!rules.evaluate_json(R"({"customer": )", error)) {
            std::cout << "\nerror: " << error.json() << "\n";
        }
        return error.is(cr::errc::format_error) ? 0 : 1;
    } catch (const cr::error &e) {
        std::cerr << cr::name(e.code()) << ": " << e.what() << "\n";
        if (!e.hint().empty()) {
            std::cerr << "hint: " << e.hint() << "\n";
        }
        return 1;
    }
}
