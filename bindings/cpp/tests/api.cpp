// The C++ API over the C ABI: handles, values, decisions, errors both ways,
// actions, grids, nodes and batches. The conformance vectors run in E.3.

#include <cleanrules.hpp>

#include <atomic>
#include <cstdint>
#include <exception>
#include <limits>
#include <stdexcept>
#include <string>
#include <thread>
#include <type_traits>
#include <unordered_set>
#include <utility>
#include <vector>

#include "check.hpp"

namespace cr = cleanrules;

// Whether `expr` throws a `type`.
#define CHECK_THROWS(expr, type)                                           \
    do {                                                                   \
        bool caught = false;                                               \
        try {                                                              \
            (void)(expr);                                                  \
        } catch (const type &) {                                           \
            caught = true;                                                 \
        } catch (...) {                                                    \
        }                                                                  \
        if (!caught) {                                                     \
            ::check::fail(__FILE__, __LINE__, #expr " throws " #type);     \
        }                                                                  \
    } while (false)

namespace {

const char *const rules_text = R"(HIT POLICY priority

RULE blocked PRIORITY 99 DESCRIPTION "blocked customers never get a discount"
IF customer.blocked
THEN discount = 0

RULE vip PRIORITY 10 TAGS loyalty
IF customer.years > 5 AND order.total > 1000
THEN discount = 0.15
DO notify_sales

RULE loyal PRIORITY 5 TAGS loyalty
IF customer.years > 2 OR customer.newsletter
THEN discount = 0.05
)";

cr::value input(std::int64_t years, double total) {
    return cr::value::map({
        {"customer", cr::value::map({{"years", years}})},
        {"order", cr::value::map({{"total", total}})},
    });
}

// Handles are move-only, values also copy.
static_assert(!std::is_copy_constructible_v<cr::ruleset>);
static_assert(std::is_nothrow_move_constructible_v<cr::ruleset>);
static_assert(!std::is_copy_constructible_v<cr::decision>);
static_assert(!std::is_copy_constructible_v<cr::actions>);
static_assert(std::is_copy_constructible_v<cr::value>);
static_assert(std::is_nothrow_move_constructible_v<cr::value>);
// Pointers other than strings, and characters, are not values.
static_assert(!std::is_constructible_v<cr::value, int *>);
static_assert(!std::is_constructible_v<cr::value, const void *>);
static_assert(!std::is_constructible_v<cr::value, wchar_t>);
static_assert(!std::is_constructible_v<cr::value, char32_t>);
static_assert(!std::is_constructible_v<cr::value, unsigned char>);
static_assert(std::is_constructible_v<cr::value, char *>);

void versions() {
    CHECK(cr::abi_version() == cr::header_abi_version);
    CHECK(!cr::version().empty());
    CHECK(cr::format_version() >= 1);
}

void reading_and_evaluating() {
    auto rules = cr::ruleset::read(rules_text, cr::format::text);
    CHECK(rules);
    CHECK(rules.size() == 3);
    CHECK(rules.rule_name(1) == "vip");
    CHECK(rules.hit_policy() == "priority");
    CHECK(rules.logic() == "boolean");
    CHECK(!rules.aggregate());
    CHECK(!rules.strict());

    auto decision = rules.evaluate(input(7, 1500));
    CHECK(decision.size() == 3);
    CHECK(!decision.fired(0));
    CHECK(decision.fired(1));
    CHECK(decision.fired(2));
    CHECK(decision.truth(0) == cr::truth::unknown);
    CHECK(decision.truth(1) == cr::truth::true_);
    CHECK(decision.selected() == std::vector<std::size_t>{1});
    CHECK(decision.rule_name(1) == "vip");
    CHECK(decision.output()["discount"].as_decimal() == "0.15");
    CHECK(decision.output_json() == R"({"discount":{"$decimal":"0.15"}})");
    CHECK(decision.outputs().size() == 1);
    CHECK(decision.rule_output(1)["discount"].as_decimal() == "0.15");
    CHECK(decision.rule_output(0).is_null());

    auto from_json = rules.evaluate_json(R"({"customer": {"years": 3}})");
    CHECK(from_json.selected() == std::vector<std::size_t>{2});

    std::string explanation = decision.explain();
    CHECK(explanation.find("vip") != std::string::npos);
    CHECK(decision.explain(cr::format::json).front() == '{');
    cr::error_info error;
    CHECK(decision.explain(error) == explanation);
    CHECK(decision.write(error) == decision.write());
    CHECK(rules.summary(error) == rules.summary());
    CHECK(rules.write(error) == rules.write());
    CHECK(!error);
    CHECK(rules.write().front() == '{');  // JSON by default
    CHECK(cr::value::null(cr::null_reason::int_overflow, error).as_null().reason ==
          cr::null_reason::int_overflow);
    CHECK(cr::grid::make(cr::value::list({"RULE", "x", "THEN y"}), error));
    std::string summary = rules.summary();
    CHECK(summary.find("loyal") != std::string::npos);
    CHECK(rules.summary(cr::format::yaml).find("rules") != std::string::npos);

    // Written and read back: the same rule set, and the decision with it.
    auto again = cr::ruleset::read(rules.write(cr::format::json), cr::format::json);
    CHECK(again.write(cr::format::text) == rules.write(cr::format::text));
    auto recorded = rules.read_decision(decision.write(), cr::format::json);
    CHECK(recorded.selected() == decision.selected());
    CHECK(recorded.write(cr::format::yaml) == decision.write(cr::format::yaml));

    auto paths = rules.field_paths();
    CHECK(paths.kind() == cr::value_type::list);
    CHECK(paths.size() == 4);
    CHECK(paths[0][0].as_string() == "customer");
    CHECK(paths[0][1].as_string() == "blocked");
}

void documents_and_limits() {
    auto doc = cr::document::read(rules_text, cr::format::text);
    CHECK(doc.content() == cr::document_content::ruleset);
    auto rules = cr::ruleset::from_document(doc);
    CHECK(rules.size() == 3);
    CHECK(doc.write(cr::format::yaml).find("vip") != std::string::npos);

    auto condition = cr::document::read(R"({"version": 1, "condition": {"op": "field", "path": ["a"]}})",
                                        cr::format::json);
    CHECK(condition.content() == cr::document_content::condition);
    CHECK_THROWS(cr::ruleset::from_document(condition), cr::format_error);

    cr::limits l;
    CHECK(l.get(cr::limit::max_nesting) > 0);
    l.set(cr::limit::max_size, 10);
    CHECK(l.get(cr::limit::max_size) == 10);
    CHECK_THROWS(cr::ruleset::read(rules_text, cr::format::text, l), cr::format_error);
    try {
        (void)cr::ruleset::read(rules_text, cr::format::text, l);
    } catch (const cr::error &e) {
        CHECK(e.code() == cr::errc::limit_exceeded);
        CHECK(e.info().is(cr::errc::format_error));
    }
    CHECK_THROWS(l.set(static_cast<cr::limit>(99), 1), cr::invalid_argument_error);
    // max_values: a million by default, carried for the bindings that read native data.
    CHECK(cr::limits().get(cr::limit::max_values) == 1000000);
    l.set(cr::limit::max_values, 12);
    CHECK(l.get(cr::limit::max_values) == 12);
    // Messages of the C ABI never name its functions.
    try {
        l.set(static_cast<cr::limit>(99), 1);
    } catch (const cr::error &e) {
        CHECK(std::string(e.what()).find("cr_") == std::string::npos);
    }
}

void values() {
    cr::value v = cr::value::record({
        {"flag", true},
        {"count", -7},
        {"big", std::uint64_t{1} << 63},
        {"ratio", 1.5},
        {"name", "héllo"},
        {"day", cr::date{2026, 11, 3}},
        {"when", cr::naive_datetime{2026, 11, 3, 9, 30, 0, 250}},
        {"at", cr::aware_datetime{0, 0, 3600, "Europe/Madrid"}},
        {"thing", cr::opaque{"socket", 42}},
        {"items", cr::value::list({1, "two", nullptr})},
        {"price", cr::value::decimal("19.90")},
        {"nothing", cr::value::null(cr::null_reason::read_error, "getter raised")},
    });
    CHECK(v.is_record());
    CHECK(v.size() == 12);
    CHECK(v["flag"].as_bool());
    CHECK(v["count"].as_int() == -7);
    CHECK(v["big"].as_null().reason == cr::null_reason::int_overflow);
    CHECK(v["ratio"].as_float() == 1.5);
    CHECK(v["name"].as_string() == "héllo");
    CHECK(v["day"].as_date().day == 3);
    CHECK(v["when"].as_naive_datetime().microsecond == 250);
    CHECK(v["at"].as_aware_datetime().zone == "Europe/Madrid");
    CHECK(v["at"].as_aware_datetime().offset_seconds == 3600);
    CHECK(v["thing"].as_opaque().type_name == "socket");
    CHECK(v["thing"].as_opaque().token == 42);
    CHECK(v["items"].size() == 3);
    CHECK(v["items"][1].as_string() == "two");
    CHECK(v["items"][2].is_null());
    CHECK(!v["items"][3]);
    CHECK(!v["missing"]);
    CHECK(v["price"].as_decimal() == "19.90");
    CHECK(v["nothing"].as_null().detail == "getter raised");
    CHECK(v.entry(1).first == "count");
    CHECK(v.entry(1).second.as_int() == -7);
    CHECK(cr::value(std::numeric_limits<double>::quiet_NaN()).is_null());

    // Copies are deep and keep what JSON loses.
    cr::value copy = v;
    CHECK(copy["thing"].as_opaque().token == 42);
    CHECK(copy["nothing"].as_null().reason == cr::null_reason::read_error);
    CHECK(copy.to_json() == v.to_json());
    cr::value moved = std::move(copy);
    CHECK(!copy);  // moved-from handles are empty
    CHECK(moved.size() == 12);
    CHECK(v["items"].clone().size() == 3);

    // JSON round trip.
    auto read = cr::value::from_json(R"({"a": [1, 2.5, "x"], "b": {"$decimal": "0.1"}})");
    CHECK(read["a"][1].as_float() == 2.5);
    CHECK(read["b"].kind() == cr::value_type::decimal);
    CHECK(cr::value::from_json(read.to_json()).to_json() == read.to_json());

    // Building in place: chained on temporaries, or step by step.
    auto built = cr::value::map().insert("a", 1).insert("b", cr::value::list_of_size(2).set(1, "x"));
    CHECK(built["b"][0].is_null());
    CHECK(built["b"][1].as_string() == "x");
    cr::value list = cr::value::list_of_size(1);
    list.set(0, 5);
    CHECK(list[0].as_int() == 5);
    // `list({...})` always means items; the sized list has its own name.
    CHECK(cr::value::list({3}).size() == 1);
    CHECK(cr::value::list({3})[0].as_int() == 3);
    CHECK(cr::value::list_of_size(3).size() == 3);
    CHECK(cr::value::list().size() == 0);
    CHECK(cr::value(static_cast<const char *>(nullptr)).is_null());
    CHECK(cr::value(std::int16_t{-200}).as_int() == -200);

    // Read from a temporary, views become owned copies.
    auto labels = cr::ruleset::read("RULE gold IF x THEN label = 'VIP'", cr::format::text);
    auto label = labels.evaluate(cr::value::map({{"x", true}})).output()["label"].as_string();
    static_assert(std::is_same_v<decltype(label), std::string>);
    CHECK(label == "VIP");
    auto item = cr::value::list({"a", "b"})[1];
    static_assert(std::is_same_v<decltype(item), cr::value>);
    CHECK(item.as_string() == "b");
    auto name = labels.evaluate(cr::value::map({{"x", true}})).rule_name(0);
    static_assert(std::is_same_v<decltype(name), std::string>);
    CHECK(name == "gold");
    auto entry = cr::value::map({{"k", 1}}).entry(0);
    static_assert(std::is_same_v<decltype(entry.first), std::string>);
    CHECK(entry.first == "k");
    // ... while views of an lvalue stay views.
    static_assert(std::is_same_v<decltype(v["name"]), cr::value_ref>);
    static_assert(std::is_same_v<decltype(v["name"].as_string()), std::string_view>);
    static_assert(std::is_same_v<decltype(labels.rule_name(0)), std::string_view>);

    // Readers check the kind, both ways.
    CHECK_THROWS(v["count"].as_string(), cr::invalid_argument_error);
    cr::error_info error;
    CHECK(v["count"].as_bool(error) == false);
    CHECK(error.code() == cr::errc::invalid_argument);
    CHECK(v["count"].as_int(error) == -7);
    CHECK(!error);  // cleared on success

    // What cannot be built throws from a constructor, and `make` reports it.
    CHECK_THROWS(cr::value(std::string("\xff")), cr::format_error);
    CHECK_THROWS(cr::value(cr::date{2026, 2, 30}), cr::invalid_argument_error);
    CHECK_THROWS(cr::value::decimal("abc"), cr::format_error);
    CHECK(!cr::value::make(cr::date{2026, 2, 30}, error));
    CHECK(error.code() == cr::errc::invalid_argument);
    CHECK(cr::value::make(cr::date{2026, 2, 28}, error));
    CHECK(!error);
    CHECK_THROWS(list.set(9, 1), cr::invalid_argument_error);
    CHECK(!list.insert("k", 1, error));
    CHECK(error);
}

void errors() {
    // The hierarchy of spec/errors.md, through the C ABI's parents.
    CHECK(cr::parent(cr::errc::missing_field) == cr::errc::evaluation_error);
    CHECK(cr::parent(cr::errc::limit_exceeded) == cr::errc::format_error);
    CHECK(cr::parent(cr::errc::format_error) == cr::errc::ok);
    CHECK(cr::is(cr::errc::type_mismatch, cr::errc::evaluation_error));
    CHECK(cr::name(cr::errc::rule_syntax_error) == "rule_syntax_error");
    CHECK(cr::name(cr::errc::invalid_argument) == "invalid_argument");

    // Throwing: the most derived class, its parents, and the details.
    try {
        (void)cr::ruleset::read("RULE r IF THEN", cr::format::text);
        CHECK(false);
    } catch (const cr::rule_syntax_error &e) {
        CHECK(e.code() == cr::errc::rule_syntax_error);
        CHECK(std::string(e.what()) == std::string(e.info().message()));
        CHECK(e.json().find("\"rule_syntax_error\"") != std::string_view::npos);
    }
    auto strict = cr::ruleset::read("STRICT\nRULE r IF a.b > 1 THEN x = 1", cr::format::text);
    CHECK(strict.strict());
    try {
        (void)strict.evaluate(cr::value::map());
        CHECK(false);
    } catch (const cr::evaluation_error &e) {
        CHECK(e.code() == cr::errc::missing_field);
        CHECK(dynamic_cast<const cr::missing_field_error *>(&e) != nullptr);
        CHECK(e.json().find(R"("path":["a","b"])") != std::string_view::npos);
    }
    CHECK_THROWS(strict.evaluate(cr::value::map()), cr::missing_field_error);
    CHECK_THROWS(strict.evaluate(cr::value::map()), std::runtime_error);

    // Not throwing: the same error in the error_info, an empty result.
    cr::error_info error;
    auto none = cr::ruleset::read("RULE r IF THEN", cr::format::text, error);
    CHECK(!none);
    CHECK(error);
    CHECK(error.code() == cr::errc::rule_syntax_error);
    CHECK(!error.message().empty());
    auto decision = strict.evaluate(cr::value::map(), error);
    CHECK(!decision);
    CHECK(error.is(cr::errc::evaluation_error));
    auto fine = strict.evaluate(cr::value::map({{"a", cr::value::map({{"b", 2}})}}), error);
    CHECK(fine);
    CHECK(!error);
    CHECK(error.message().empty());

    // A moved-from handle is a misuse of the ABI, not a crash.
    cr::ruleset gone = std::move(strict);
    CHECK_THROWS(strict.evaluate(cr::value::map()), cr::invalid_argument_error);
    CHECK(gone);

    // Several outputs ask for `outputs`.
    auto collect = cr::ruleset::read("HIT POLICY collect\nRULE a IF x THEN y = 1\n"
                                     "RULE b IF x THEN y = 2",
                                     cr::format::text);
    auto both = collect.evaluate(cr::value::map({{"x", true}}));
    CHECK_THROWS(both.output(), cr::hit_policy_error);
    CHECK(both.outputs().size() == 2);
}

void actions() {
    auto rules = cr::ruleset::read(rules_text, cr::format::text);
    std::vector<std::string> seen;
    cr::actions registered;
    registered.add("notify_sales", [&](cr::action_call &call) {
        seen.push_back(std::string(call.rule_name()));
        CHECK(call.rule() == 1);
        CHECK(call.input()["order"]["total"].as_float() == 1500);
        CHECK(call.decision().fired(1));
    });
    rules.check_actions(registered);
    auto decision = rules.run(registered, input(7, 1500));
    CHECK(seen == std::vector<std::string>{"vip"});
    CHECK(decision.selected() == std::vector<std::size_t>{1});

    // An unregistered action: nothing runs.
    cr::actions empty;
    CHECK_THROWS(rules.check_actions(empty), cr::registry_error);
    CHECK_THROWS(rules.run(empty, input(7, 1500)), cr::registry_error);
    CHECK_THROWS(registered.add("notify_sales", [](cr::action_call &) {}),
                 cr::invalid_argument_error);
    CHECK_THROWS(registered.add("", [](cr::action_call &) {}), cr::invalid_argument_error);

    // An action that fails: the decision and the reason.
    cr::actions failing;
    failing.add("notify_sales", [](cr::action_call &call) { call.fail("mail server down"); });
    try {
        rules.run(failing, input(7, 1500));
        CHECK(false);
    } catch (const cr::action_error &e) {
        CHECK(std::string(e.what()).find("mail server down") != std::string::npos);
        CHECK(e.decision());
        CHECK(e.decision().fired(1));
        CHECK(e.json().find(R"("cause":"mail server down")") != std::string_view::npos);
    }
    cr::error_info error;
    CHECK(!rules.run(failing, input(7, 1500), error));
    CHECK(error.code() == cr::errc::action_error);
    CHECK(error.decision().selected() == std::vector<std::size_t>{1});
    CHECK(!error.cause());
    // The decision is a copy: it outlives the error_info's next use.
    cr::decision kept = error.decision();
    (void)rules.evaluate(input(1, 0), error);
    CHECK(!error);
    CHECK(!error.decision());
    CHECK(kept.fired(1));
    CHECK(kept.explain().find("vip") != std::string::npos);

    // An action that throws: its exception is nested in the action_error.
    cr::actions throwing;
    throwing.add("notify_sales",
                 [](cr::action_call &) { throw std::out_of_range("no such account"); });
    try {
        rules.run(throwing, input(7, 1500));
        CHECK(false);
    } catch (const cr::action_error &e) {
        CHECK(std::string(e.what()).find("no such account") != std::string::npos);
        bool nested = false;
        try {
            std::rethrow_if_nested(e);
        } catch (const std::out_of_range &inner) {
            nested = std::string(inner.what()) == "no such account";
        }
        CHECK(nested);
    }
    CHECK(!rules.run(throwing, input(7, 1500), error));
    CHECK(error.cause() != nullptr);

    // A non-std exception, and an action that runs the rules again.
    cr::actions odd;
    odd.add("notify_sales", [](cr::action_call &) { throw 42; });
    CHECK_THROWS(rules.run(odd, input(7, 1500)), cr::action_error);
    int depth = 0;
    cr::actions nested;
    nested.add("notify_sales", [&](cr::action_call &call) {
        if (++depth == 1) {
            // The inner run fails, and its action_error fails the outer one.
            rules.run(throwing, call.input().clone());
        }
    });
    try {
        rules.run(nested, input(7, 1500));
        CHECK(false);
    } catch (const cr::action_error &outer) {
        bool inner_action_error = false;
        try {
            std::rethrow_if_nested(outer);
        } catch (const cr::action_error &inner) {
            inner_action_error = true;
            bool innermost = false;
            try {
                std::rethrow_if_nested(inner);
            } catch (const std::out_of_range &) {
                innermost = true;
            }
            CHECK(innermost);
        }
        CHECK(inner_action_error);
    }
    CHECK(depth == 1);

    // A finished registry runs on several threads at once.
    std::atomic<int> calls{0};
    cr::actions counting;
    counting.add("notify_sales", [&](cr::action_call &) { ++calls; });
    std::vector<std::thread> threads;
    for (int t = 0; t < 4; ++t) {
        threads.emplace_back([&] {
            for (int i = 0; i < 25; ++i) {
                rules.run(counting, input(7, 1500));
            }
        });
    }
    for (auto &thread : threads) {
        thread.join();
    }
    CHECK(calls == 100);
}

void grids_nodes_and_batches() {
    auto table = cr::grid::make(cr::value::list({"RULE", "customer.years", "THEN discount"}),
                                {"priority", {}, {}, false});
    table.push_row(cr::value::list({"vip", "> 5", cr::value::decimal("0.15")}))
        .push_row(cr::value::list({"loyal", "> 2", cr::value::decimal("0.05")}));
    auto doc = table.finish();
    CHECK(!table);
    CHECK(doc.content() == cr::document_content::decision_table);
    CHECK(doc.write(cr::format::csv).find("vip") != std::string::npos);
    auto rules = cr::ruleset::from_document(doc);
    CHECK(rules.hit_policy() == "priority");
    CHECK(rules.size() == 2);

    // A failed row spoils the grid.
    auto bad = cr::grid::make(cr::value::list({"RULE", "x", "THEN y"}));
    CHECK_THROWS(bad.push_row(cr::value::list({"r", "> > >", 1})), cr::rule_syntax_error);
    CHECK_THROWS(bad.push_row(cr::value::list({"r", "> 1", 1})), cr::invalid_argument_error);
    CHECK_THROWS(bad.finish(), cr::invalid_argument_error);

    auto condition = cr::node::read("customer.years > 5", cr::format::text);
    CHECK(condition.write(cr::format::text) == "customer.years > 5");
    std::string tree = condition.evaluate_tree(input(7, 0));
    CHECK(tree.find(R"("truth":true)") != std::string::npos);
    auto price = cr::node::read("order.total * 0.9", cr::format::text);
    CHECK(price.evaluate_value_tree(input(7, 100)).find("90") != std::string::npos);
    CHECK_THROWS(price.evaluate_tree(input(7, 100)), cr::expression_error);
    CHECK_THROWS(condition.evaluate_tree(input(7, 0), "nope"), cr::registry_error);
    cr::error_info error;
    CHECK(condition.evaluate_tree(input(7, 0), "nope", error).empty());
    CHECK(error.code() == cr::errc::registry_error);
    CHECK(!condition.evaluate_tree(input(7, 0), "boolean", true, error).empty());
    CHECK(!error);
    CHECK_THROWS(cr::node::read("a,b", cr::format::csv), cr::unsupported_error);
    // The paths a node reads, in preorder and each once.
    auto twice = cr::node::read("a.b[1] > 5 OR (x = 1 AND a.b[1] < 2)", cr::format::text);
    auto node_paths = twice.field_paths();
    CHECK(node_paths.size() == 2);
    CHECK(node_paths[0].size() == 3);
    CHECK(node_paths[0][2].as_int() == 1);
    CHECK(node_paths[1][0].as_string() == "x");
    CHECK(twice.field_paths(error).size() == 2);
    CHECK(!error);
    CHECK(cr::node().field_paths(error).kind() == cr::value_type::none);
    CHECK(error.code() == cr::errc::invalid_argument);

    auto all = rules.evaluate_many(cr::value::list({input(7, 0), input(3, 0), input(1, 0)}));
    CHECK(all.size() == 3);
    std::vector<std::size_t> winners;
    for (cr::decision_ref d : all) {
        auto selected = d.selected();
        winners.push_back(selected.empty() ? 99 : selected[0]);
    }
    CHECK(winners == (std::vector<std::size_t>{0, 1, 99}));
    CHECK(!all[3]);
    try {
        (void)rules.evaluate_many(cr::value::map());
        CHECK(false);
    } catch (const cr::evaluation_error &e) {
        CHECK(e.code() == cr::errc::evaluation_error);
    }
}

void building() {
    // Conditions, rules and rule sets built in code, checked by the core.
    auto years = cr::node::field(cr::value::list({"customer", "years"}));
    auto total = cr::node::field(cr::value::list({"order", "total"}));
    auto old = cr::node::compare("gt", years, cr::node::constant(5));
    // Named nodes and temporaries mix.
    auto vip_when = cr::node::and_({old, cr::node::compare("gt", total, cr::node::constant(1000))});
    CHECK(vip_when.write(cr::format::text) == "customer.years > 5 AND order.total > 1000");
    CHECK(vip_when.tag() == "and");
    CHECK(vip_when.kind() == "condition");
    CHECK(years.kind() == "any");
    auto children = vip_when.children();
    CHECK(children.size() == 2);
    CHECK(children[0].same_as(cr::node::compare("gt", years, cr::node::constant(5))));
    CHECK(children[0].hash() == cr::node::compare("gt", years, cr::node::constant(5)).hash());
    CHECK(!children[0].same_as(children[1]));
    CHECK(years.detail()[1].as_string() == "years");
    auto country = cr::node::field(cr::value::list({"country"}));
    auto in = cr::node::is_in(country, cr::value::list({"PT", "ES"}));
    CHECK(in.write(cr::format::text) == "country IN (\"ES\", \"PT\")");
    CHECK(cr::node::between(years, cr::node::constant(1), cr::node::constant(3), "left").detail()
              .as_string() == "left");
    CHECK(cr::node::not_(cr::node::is_null(years)).write(cr::format::text) ==
          "NOT customer.years IS NULL");
    CHECK(cr::node::string_test("starts_with", country, "E").tag() == "starts_with");
    CHECK(cr::node::matches(country, "^E").tag() == "matches");
    auto price = cr::node::neg(cr::node::abs(cr::node::arith("mul", total, cr::node::constant(2))));
    CHECK(price.kind() == "value");
    CHECK_THROWS(cr::node::compare("gt", vip_when, years), cr::expression_error);
    CHECK_THROWS(cr::node::or_({years}), cr::expression_error);
    CHECK_THROWS(cr::node::constant(cr::value::list({1})), cr::expression_error);
    CHECK_THROWS(cr::node::compare("near", years, total), cr::invalid_argument_error);
    cr::error_info error;
    CHECK(!cr::node::matches(country, "(?=a)", error));
    CHECK(error.code() == cr::errc::expression_error);

    auto vip = cr::rule::make("vip", vip_when)
                   .with_payload("discount", cr::node::constant(cr::value::decimal("0.15")))
                   .with_priority(10)
                   .with_tags(cr::value::list({"b", "a", "b"}))
                   .with_description("big spenders")
                   .with_metadata(cr::value::map({{"owner", "sales"}}))
                   .with_action("notify");
    CHECK(vip.tags().size() == 2);
    CHECK(vip.tags()[0].as_string() == "a");
    CHECK(vip.metadata()["owner"].as_string() == "sales");
    auto loyal = cr::rule::make("loyal", cr::node::compare("gt", years, cr::node::constant(2)))
                     .with_payload("discount", cr::node::constant(cr::value::decimal("0.05")));
    CHECK_THROWS(cr::rule::make("", vip_when), cr::rule_definition_error);
    CHECK_THROWS(loyal.with_payload("ok", vip_when), cr::rule_definition_error);
    auto rules = cr::ruleset::make({vip, loyal}, cr::ruleset_settings{"priority", {}, {}, false});
    std::unordered_set<cr::node> nodes;
    nodes.insert(cr::node::compare("gt", years, cr::node::constant(5)));
    nodes.insert(cr::node::compare("gt", years, cr::node::constant(5)));
    nodes.insert(years.children().empty() ? cr::node::constant(1) : cr::node::constant(2));
    CHECK(nodes.size() == 2);
    CHECK(nodes.count(old) == 1);
    CHECK(rules.hit_policy() == "priority");
    CHECK(rules.rule_position("loyal") == 1);
    CHECK_THROWS(rules.rule_position("nope"), cr::rule_not_found_error);
    CHECK_THROWS(cr::ruleset::make({vip, vip}), cr::rule_definition_error);
    CHECK(!cr::ruleset::make({vip}, {"best", {}, {}, false}, error));
    CHECK(error.code() == cr::errc::rule_definition_error);
    CHECK(rules.summary(std::vector<bool>{false, true}, cr::format::json).find(R"("action":true)") !=
          std::string::npos);
    auto decision = rules.evaluate(input(3, 1200));
    CHECK(decision.selected() == std::vector<std::size_t>{1});
    auto explanation = decision.explanation();
    CHECK(explanation["hit_policy"].as_string() == "priority");
    CHECK(explanation["rules"][0]["status"].as_string() == "not_fired");
    CHECK(explanation["rules"][1]["requirement"]["holds"].as_bool());
    CHECK(decision.explain().find(explanation["rules"][1]["text"].as_string()) != std::string::npos);
}

void rules_and_documents() {
    // Rules read back from rule sets, documents and text, with all they hold.
    auto rules = cr::ruleset::read(
        "HIT POLICY priority\n"
        "RULE vip PRIORITY 10 DESCRIPTION \"big\"\n"
        "IF customer.years > 5 THEN discount = 0.15, label = \"VIP\" DO notify\n"
        "RULE plain IF true\n",
        cr::format::text);
    auto vip = rules.rule_at(0);
    CHECK(vip.name() == "vip");
    CHECK(vip.priority() == 10);
    CHECK(vip.description() == std::optional<std::string>("big"));
    CHECK(vip.action_name() == std::optional<std::string>("notify"));
    CHECK(cr::rule::from_text("RULE t IF x").name() == "t");  // owned from a temporary
    CHECK(vip.when().write(cr::format::text) == "customer.years > 5");
    auto payload = vip.payload();
    CHECK(payload.size() == 2);
    CHECK(payload[0].first == "discount");
    CHECK(payload[1].second.write(cr::format::text) == "\"VIP\"");
    auto plain = rules.rule_at(1);
    CHECK(!plain.description() && !plain.action_name() && plain.payload().empty());
    cr::rule empty;
    CHECK_THROWS(empty.name(), cr::invalid_argument_error);
    cr::error_info missing;
    CHECK(empty.name(missing).empty() && missing.code() == cr::errc::invalid_argument);
    CHECK_THROWS(rules.rule_at(2), cr::invalid_argument_error);
    cr::error_info error;
    CHECK(!rules.rule_at(2, error) && error.code() == cr::errc::invalid_argument);

    // Documents made of a rule or a condition, and what a document holds.
    auto written = cr::document::from_rule(vip).write(cr::format::text);
    CHECK(cr::document::read(written, cr::format::text).as_rule().name() == "vip");
    auto condition = cr::document::read("age >= 18", cr::format::text).as_condition();
    CHECK(condition.write(cr::format::text) == "age >= 18");
    CHECK(cr::document::from_condition(condition).write() ==
          cr::document::read("age >= 18", cr::format::text).write());
    auto price = cr::node::arith("mul", cr::node::field(cr::value::list({"price"})), cr::node::constant(2));
    CHECK_THROWS(cr::document::from_condition(price), cr::expression_error);
    CHECK_THROWS(cr::document::read("age >= 18", cr::format::text).as_rule(), cr::invalid_argument_error);

    // One rule from text.
    auto unnamed = cr::rule::from_text("IF x > 1 THEN y = 2");
    CHECK(unnamed.name() == "rule_1");
    CHECK_THROWS(cr::rule::from_text("x > 1"), cr::rule_syntax_error);
    cr::error_info syntax;
    CHECK(!cr::rule::from_text("RULE a IF x RULE b IF y", syntax) &&
          syntax.code() == cr::errc::rule_syntax_error);
}

}  // namespace

int main() {
    versions();
    reading_and_evaluating();
    documents_and_limits();
    values();
    errors();
    actions();
    grids_nodes_and_batches();
    building();
    rules_and_documents();
    return check::report("api");
}
