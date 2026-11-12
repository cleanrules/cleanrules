// The conformance vectors (conformance/README.md) through the C++ API. Every
// vector runs twice, as in each binding:
//
// - `json:<id>`: documents, inputs and results cross as JSON text, through the
//   reading and writing entry points, with the overloads that throw. `read`
//   vectors run a second time with the error_info overloads.
// - `native:<id>`, for every kind but `read`: the input is built as a
//   `cleanrules::value` from C++ data with the value constructors, checked
//   against what the library reads from the same JSON and against a copy
//   rebuilt with the value readers, and evaluated with the error_info
//   overloads; decisions are also read back with the decision readers.
//
// The runner only drives the library and compares: results are parsed with
// json.hpp and compared with the vectors as trees, never with the library's
// own JSON. Rule sets and nodes are read from their JSON in both runs, because
// the C++ API, like the C ABI, builds them from documents and text only.

#include <cleanrules.hpp>

#include <algorithm>
#include <charconv>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <exception>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <limits>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <system_error>
#include <utility>
#include <vector>

#include "json.hpp"

#ifndef CLEANRULES_VECTORS
#error "CMake passes the vectors' directory as CLEANRULES_VECTORS"
#endif

namespace cr = cleanrules;

namespace {

// Vectors are only ever added: the count when this runner was written.
constexpr std::size_t minimum_vectors = 2508;

// Which overloads a run calls.
enum class mode { throwing, info };

// A call that failed: the error's JSON form (spec/errors.md).
struct failure {
    json::value error;
};

// A vector the runner cannot run as written, or an answer that breaks the
// API's own promises: the vector fails with this message.
class vector_error : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

// The error of a failed call, with its code checked against its JSON form.
failure failure_of(const cr::error_info &info) {
    json::value error = json::parse(info.json());
    const std::string *code = error.string_at("code");
    if (code == nullptr || *code != cr::name(info.code())) {
        throw vector_error("the error " + std::string(info.json()) + " has the code " +
                           std::string(cr::name(info.code())));
    }
    return failure{std::move(error)};
}

void ok(const cr::error_info &info) {
    if (info) {
        throw failure_of(info);
    }
}

// Call the overload that throws, or the one that takes an error_info; either
// way, a failure is thrown as a `failure`.
template <class Throwing, class Info>
auto call(mode m, Throwing &&throwing, Info &&with_info) {
    if (m == mode::throwing) {
        try {
            return throwing();
        } catch (const cr::error &e) {
            throw failure_of(e.info());
        }
    }
    cr::error_info error;
    auto out = with_info(error);
    ok(error);
    return out;
}

// `CALL(m, f, args...)` calls `f(args...)` or `f(args..., error)`; `CALL0`,
// `f()` or `f(error)`.
#define CALL(mode_, fn_, ...)                                  \
    call((mode_), [&] { return fn_(__VA_ARGS__); },            \
         [&](cr::error_info &error_) { return fn_(__VA_ARGS__, error_); })
#define CALL0(mode_, fn_) \
    call((mode_), [&] { return fn_(); }, [&](cr::error_info &error_) { return fn_(error_); })

// --- Comparing ---------------------------------------------------------------

// What a step gave: its JSON, or the error of the call that failed.
struct outcome {
    std::optional<json::value> result;
    std::optional<json::value> error;
};

template <class Step>
outcome attempt(Step &&step) {
    try {
        return outcome{step(), std::nullopt};
    } catch (const failure &failed) {
        return outcome{std::nullopt, failed.error};
    }
}

// The fields of an error that are compared (spec/errors.md, *Comparing
// errors*): for reading, those the vector lists, as the core's runner does;
// for evaluating, the compared context fields.
enum class fields { read, evaluate };

const std::vector<std::string_view> evaluate_fields = {"code",  "rule", "path",
                                                       "types", "rules", "row"};

// An object with only these members.
json::value only(const json::value &v, const std::vector<std::string_view> &keys) {
    json::value out;
    out.kind = json::type::object;
    for (const auto &member : v.members) {
        for (std::string_view key : keys) {
            if (member.first == key) {
                out.members.push_back(member);
                break;
            }
        }
    }
    return out;
}

// The same members, whatever their order.
bool same_members(const json::value &actual, const json::value &wanted) {
    if (actual.members.size() != wanted.members.size()) {
        return false;
    }
    for (const auto &member : wanted.members) {
        const json::value *found = actual.get(member.first);
        if (found == nullptr || !json::same(*found, member.second)) {
            return false;
        }
    }
    return true;
}

std::string compare_error(const json::value &actual, const json::value &wanted, fields which) {
    std::vector<std::string_view> keys;
    if (which == fields::read) {
        for (const auto &member : wanted.members) {
            keys.push_back(member.first);
        }
    } else {
        keys = evaluate_fields;
    }
    json::value got = only(actual, keys);
    json::value want = only(wanted, keys);
    if (same_members(got, want)) {
        return {};
    }
    return "error " + json::write(got) + ", expected " + json::write(want);
}

const json::value &need(const json::value &v, std::string_view key) {
    const json::value *found = v.get(key);
    if (found == nullptr) {
        throw vector_error("no `" + std::string(key) + "` in " + json::write(v));
    }
    return *found;
}

// Compare what a step gave with the vector's `key`, or with its error.
std::string compare(const outcome &actual, const json::value &expect, std::string_view key,
                    fields which) {
    const json::value *error = expect.get("error");
    if (actual.result && error == nullptr) {
        json::value got = json::strip_detail(*actual.result);
        json::value want = json::strip_detail(need(expect, key));
        if (json::same(got, want)) {
            return {};
        }
        return "\n  got  " + json::write(got) + "\n  want " + json::write(want);
    }
    if (actual.result) {
        return "got " + json::write(*actual.result) + ", expected the error " +
               json::write(*error);
    }
    if (error == nullptr) {
        return "unexpected error " + json::write(*actual.error);
    }
    return compare_error(*actual.error, *error, which);
}

// The content of a document the library wrote, such as its `decision`.
json::value content(const std::string &document, std::string_view key) {
    return need(json::parse(document), key);
}

std::string compare_text(std::string_view what, const std::string &got, const std::string &want) {
    if (got == want) {
        return {};
    }
    return std::string(what) + " " + json::quote(got) + ", expected " + json::quote(want);
}

// --- Reading -----------------------------------------------------------------

std::optional<cr::limits> limits_of(const json::value &given) {
    const json::value *wanted = given.get("limits");
    if (wanted == nullptr) {
        return std::nullopt;
    }
    std::optional<cr::limits> out(std::in_place);
    for (const auto &member : wanted->members) {
        cr::limit which = cr::limit::max_size;
        if (member.first == "max_size") {
            which = cr::limit::max_size;
        } else if (member.first == "max_nesting") {
            which = cr::limit::max_nesting;
        } else if (member.first == "max_depth") {
            which = cr::limit::max_depth;
        } else if (member.first == "max_table_size") {
            which = cr::limit::max_table_size;
        } else {
            throw vector_error("unknown limit " + member.first);
        }
        unsigned long long n = 0;
        const std::string &text = member.second.text;
        auto read = std::from_chars(text.data(), text.data() + text.size(), n);
        if (!member.second.is_integer() || read.ec != std::errc() ||
            read.ptr != text.data() + text.size()) {
            throw vector_error("the limit " + member.first + " is not a size");
        }
        cr::error_info error;
        out->set(which, static_cast<std::size_t>(n), error);
        ok(error);
    }
    return out;
}

// The rule set of a vector, as a rule set document.
std::string ruleset_document(const json::value &given) {
    return R"({"version":1,"ruleset":)" + json::write(need(given, "ruleset")) + "}";
}

// The rule set read directly (JSON runs) or through a document (native runs).
cr::ruleset ruleset_of(const json::value &given, mode m) {
    std::string text = ruleset_document(given);
    if (m == mode::throwing) {
        return CALL(m, cr::ruleset::read, text, cr::format::json);
    }
    cr::document document = CALL(m, cr::document::read, text, cr::format::json);
    return CALL(m, cr::ruleset::from_document, document);
}

// A table from a vector's grid, through `cleanrules::grid`.
cr::document read_grid(const json::value &grid, const std::optional<cr::limits> &limits,
                       mode m) {
    cr::table_settings settings;
    if (const std::string *policy = grid.string_at("hit_policy")) {
        settings.hit_policy = *policy;
    }
    if (const std::string *aggregate = grid.string_at("aggregate")) {
        settings.aggregate = *aggregate;
    }
    if (const std::string *logic = grid.string_at("logic")) {
        settings.logic = *logic;
    }
    settings.strict = grid.flag("strict");
    std::string header_text = json::write(need(grid, "header"));
    cr::value header = CALL(m, cr::value::from_json, header_text);
    cr::grid table = limits ? CALL(m, cr::grid::make, header, settings, *limits)
                            : CALL(m, cr::grid::make, header, settings);
    for (const json::value &cells : need(grid, "rows").items) {
        std::string row_text = json::write(cells);
        cr::value row = CALL(m, cr::value::from_json, row_text);
        call(
            m,
            [&] {
                table.push_row(row);
                return true;
            },
            [&](cr::error_info &error) { return table.push_row(row, error); });
    }
    return CALL0(m, table.finish);
}

// A `read` vector: documents in every format, grids, and decisions read with
// their rule set, written back in the forms the vector expects.
std::string run_read(const json::value &given, const json::value &expect, mode m) {
    std::string source;
    cr::format fmt = cr::format::json;
    if (const std::string *text = given.string_at("text")) {
        source = *text;
    } else if (const json::value *document = given.get("document")) {
        source = json::write(*document);
    } else if (const std::string *yaml = given.string_at("yaml")) {
        source = *yaml;
        fmt = cr::format::yaml;
    } else if (const std::string *rules = given.string_at("rules_text")) {
        source = *rules;
        fmt = cr::format::text;
    } else if (const std::string *csv = given.string_at("table_csv")) {
        source = *csv;
        fmt = cr::format::csv;
    } else if (given.get("table_grid") == nullptr) {
        throw vector_error("nothing to read");
    }
    std::optional<cr::limits> limits = limits_of(given);

    if (given.get("ruleset") != nullptr) {
        cr::ruleset rules = ruleset_of(given, m);
        outcome read = attempt([&] {
            cr::decision decision = limits ? CALL(m, rules.read_decision, source, fmt, *limits)
                                           : CALL(m, rules.read_decision, source, fmt);
            return json::parse(CALL0(m, decision.write));
        });
        return compare(read, expect, "document", fields::read);
    }

    cr::document document;
    try {
        if (const json::value *grid = given.get("table_grid")) {
            document = read_grid(*grid, limits, m);
        } else if (limits) {
            document = CALL(m, cr::document::read, source, fmt, *limits);
        } else {
            document = CALL(m, cr::document::read, source, fmt);
        }
    } catch (const failure &failed) {
        return compare(outcome{std::nullopt, failed.error}, expect, "document", fields::read);
    }

    if (const json::value *error = expect.get("error")) {
        // Read, but not writable in the form the vector names.
        const std::string *form = given.string_at("write");
        cr::format target = cr::format::json;
        if (form != nullptr && *form == "text") {
            target = cr::format::text;
        } else if (form != nullptr && *form == "csv") {
            target = cr::format::csv;
        } else {
            return "read a document, expected the error " + json::write(*error);
        }
        try {
            std::string written = CALL(m, document.write, target);
            return "wrote " + json::quote(written) + ", expected the error " +
                   json::write(*error);
        } catch (const failure &failed) {
            return compare_error(failed.error, *error, fields::read);
        }
    }
    if (expect.get("document") != nullptr) {
        outcome written = attempt([&] { return json::parse(CALL0(m, document.write)); });
        if (std::string issue = compare(written, expect, "document", fields::read);
            !issue.empty()) {
            return issue;
        }
    }
    if (const std::string *printed = expect.string_at("printed")) {
        if (std::string issue =
                compare_text("printed", CALL(m, document.write, cr::format::text), *printed);
            !issue.empty()) {
            return issue;
        }
    }
    if (const std::string *csv = expect.string_at("csv")) {
        if (std::string issue =
                compare_text("wrote the CSV", CALL(m, document.write, cr::format::csv), *csv);
            !issue.empty()) {
            return issue;
        }
    }
    if (expect.get("compiled") != nullptr) {
        outcome compiled = attempt([&] {
            cr::ruleset rules = CALL(m, cr::ruleset::from_document, document);
            return content(CALL0(m, rules.write), "ruleset");
        });
        return compare(compiled, expect, "compiled", fields::read);
    }
    return {};
}

// --- Native values -------------------------------------------------------------

bool all_digits(std::string_view s) {
    if (s.empty()) {
        return false;
    }
    for (char c : s) {
        if (c < '0' || c > '9') {
            return false;
        }
    }
    return true;
}

// An int from `-?[0-9]+`: beyond 64 bits it is a null with the reason
// `int_overflow`, as data reads it (spec/json-format.md, *Numbers*).
cr::value integer(std::string_view text) {
    std::string_view digits = !text.empty() && text[0] == '-' ? text.substr(1) : text;
    if (!all_digits(digits)) {
        throw vector_error("the int " + std::string(text) + " is not an integer literal");
    }
    std::int64_t n = 0;
    auto read = std::from_chars(text.data(), text.data() + text.size(), n);
    if (read.ec == std::errc::result_out_of_range) {
        cr::error_info error;
        cr::value out = cr::value::null(cr::null_reason::int_overflow, error);
        ok(error);
        return out;
    }
    if (read.ec != std::errc() || read.ptr != text.data() + text.size()) {
        throw vector_error("the int " + std::string(text) + " cannot be read");
    }
    return cr::value(n);
}

// A float from a number literal; one that does not fit a double has no native
// form (the format rejects it).
cr::value floating(const std::string &text) {
    double f = std::strtod(text.c_str(), nullptr);
    if (!std::isfinite(f)) {
        throw vector_error("the float " + text + " does not fit a double");
    }
    return cr::value(f);
}

// `width` digits of `s` at `pos` as a number, or -1.
std::int32_t number_at(std::string_view s, std::size_t pos, std::size_t width) {
    if (pos + width > s.size() || !all_digits(s.substr(pos, width))) {
        return -1;
    }
    std::int32_t out = 0;
    for (std::size_t i = pos; i < pos + width; ++i) {
        out = out * 10 + (s[i] - '0');
    }
    return out;
}

cr::date date_of(std::string_view s) {
    cr::date d{number_at(s, 0, 4), number_at(s, 5, 2), number_at(s, 8, 2)};
    if (s.size() != 10 || s[4] != '-' || s[7] != '-' || d.year < 0 || d.month < 0 || d.day < 0) {
        throw vector_error("the date " + std::string(s) + " is not YYYY-MM-DD");
    }
    return d;
}

// Days from 1970-01-01 to a day of the proleptic Gregorian calendar (Howard
// Hinnant's days_from_civil).
std::int64_t days_from_civil(std::int64_t y, std::int64_t m, std::int64_t d) {
    if (m <= 2) {
        --y;
    }
    const std::int64_t era = (y >= 0 ? y : y - 399) / 400;
    const std::int64_t yoe = y - era * 400;
    const std::int64_t doy = (153 * (m > 2 ? m - 3 : m + 9) + 2) / 5 + d - 1;
    const std::int64_t doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    return era * 146097 + doe - 719468;
}

// A `$datetime`: naive, or an instant with its offset and, in brackets, its
// zone (spec/json-format.md, *Dates and datetimes*), built with the C++ types.
cr::value datetime(std::string_view s) {
    auto bad = [&] { return vector_error("the datetime " + std::string(s) + " cannot be read"); };
    if (s.size() < 19 || s[10] != 'T' || s[13] != ':' || s[16] != ':') {
        throw bad();
    }
    cr::date day = date_of(s.substr(0, 10));
    cr::naive_datetime naive{day.year,          day.month,         day.day,
                             number_at(s, 11, 2), number_at(s, 14, 2), number_at(s, 17, 2),
                             0};
    if (naive.hour < 0 || naive.minute < 0 || naive.second < 0) {
        throw bad();
    }
    std::size_t pos = 19;
    if (pos < s.size() && s[pos] == '.') {
        std::size_t start = ++pos;
        while (pos < s.size() && s[pos] >= '0' && s[pos] <= '9') {
            ++pos;
        }
        std::size_t width = pos - start;
        if (width == 0 || width > 6) {
            throw bad();
        }
        naive.microsecond = number_at(s, start, width);
        for (std::size_t i = width; i < 6; ++i) {
            naive.microsecond *= 10;
        }
    }
    cr::error_info error;
    if (pos == s.size()) {
        cr::value out = cr::value::make(naive, error);
        ok(error);
        return out;
    }
    std::int32_t offset = 0;
    if (s[pos] == 'Z') {
        ++pos;
    } else if (s[pos] == '+' || s[pos] == '-') {
        std::int32_t sign = s[pos] == '-' ? -1 : 1;
        std::int32_t hours = number_at(s, pos + 1, 2);
        std::int32_t minutes = number_at(s, pos + 4, 2);
        if (hours < 0 || minutes < 0 || pos + 3 >= s.size() || s[pos + 3] != ':') {
            throw bad();
        }
        offset = hours * 3600 + minutes * 60;
        pos += 6;
        if (pos < s.size() && s[pos] == ':') {
            std::int32_t seconds = number_at(s, pos + 1, 2);
            if (seconds < 0) {
                throw bad();
            }
            offset += seconds;
            pos += 3;
        }
        offset *= sign;
    } else {
        throw bad();
    }
    std::string zone;
    if (pos < s.size()) {
        if (s[pos] != '[' || s.back() != ']') {
            throw bad();
        }
        zone = std::string(s.substr(pos + 1, s.size() - pos - 2));
    }
    // The instant: the local time less its offset.
    std::int64_t seconds = days_from_civil(naive.year, naive.month, naive.day) * 86400 +
                           naive.hour * 3600 + naive.minute * 60 + naive.second - offset;
    cr::value out = cr::value::make(
        cr::aware_datetime{seconds, naive.microsecond, offset, std::move(zone)}, error);
    ok(error);
    return out;
}

cr::value native(const json::value &v);

// A map, its entries in order.
cr::value map_of(const json::value &object) {
    cr::value out = cr::value::map();
    cr::error_info error;
    for (const auto &member : object.members) {
        out.insert(member.first, native(member.second), error);
        ok(error);
    }
    return out;
}

// A value of the JSON format (spec/json-format.md, *Values*) as native data,
// built with the C++ constructors: never through the library's JSON reader.
cr::value native(const json::value &v) {
    cr::error_info error;
    switch (v.kind) {
    case json::type::null:
        return cr::value();
    case json::type::boolean:
        return cr::value(v.boolean);
    case json::type::number:
        return v.is_integer() ? integer(v.text) : floating(v.text);
    case json::type::string: {
        cr::value out = cr::value::make(v.text, error);
        ok(error);
        return out;
    }
    case json::type::array: {
        cr::value out = cr::value::list_of_size(v.items.size());
        for (std::size_t i = 0; i < v.items.size(); ++i) {
            out.set(i, native(v.items[i]), error);
            ok(error);
        }
        return out;
    }
    case json::type::object:
        break;
    }
    bool typed = false;
    for (const auto &member : v.members) {
        typed = typed || (!member.first.empty() && member.first[0] == '$');
    }
    if (!typed) {
        return map_of(v);
    }
    if (v.members.size() != 1) {
        throw vector_error("a typed value with other members: " + json::write(v));
    }
    const std::string &tag = v.members[0].first;
    const json::value &inner = v.members[0].second;
    const bool text = inner.is(json::type::string);
    if (tag == "$int" && (inner.is_integer() || text)) {
        // Leading zeros are allowed in the string form.
        return integer(inner.text);
    }
    if (tag == "$float" && inner.is(json::type::number)) {
        return floating(inner.text);
    }
    if (tag == "$float" && text && (inner.text == "inf" || inner.text == "-inf")) {
        double infinity = std::numeric_limits<double>::infinity();
        return cr::value(inner.text == "inf" ? infinity : -infinity);
    }
    if (tag == "$decimal" && text) {
        cr::value out = cr::value::decimal(inner.text, error);
        ok(error);
        return out;
    }
    if (tag == "$date" && text) {
        cr::value out = cr::value::make(date_of(inner.text), error);
        ok(error);
        return out;
    }
    if (tag == "$datetime" && text) {
        return datetime(inner.text);
    }
    if (tag == "$map" && inner.is(json::type::object)) {
        return map_of(inner);
    }
    if (tag == "$opaque" && text) {
        // A native value without an equivalent: its type's name and a token.
        cr::value out = cr::value::make(cr::opaque{inner.text, 0}, error);
        ok(error);
        return out;
    }
    throw vector_error("no native form for " + json::write(v));
}

// A copy of a value read with the readers and rebuilt with the constructors,
// as a program that walks a value would.
cr::value rebuild(cr::value_ref v) {
    cr::error_info error;
    cr::value out;
    switch (v.kind()) {
    case cr::value_type::none:
        throw vector_error("an empty value");
    case cr::value_type::null: {
        cr::null_info null = v.as_null(error);
        ok(error);
        out = cr::value::null(null.reason, null.detail, error);
        break;
    }
    case cr::value_type::bool_:
        out = cr::value(v.as_bool(error));
        break;
    case cr::value_type::int_:
        out = cr::value(v.as_int(error));
        break;
    case cr::value_type::float_:
        out = cr::value(v.as_float(error));
        break;
    case cr::value_type::decimal: {
        std::string digits = v.as_decimal(error);
        ok(error);
        out = cr::value::decimal(digits, error);
        break;
    }
    case cr::value_type::string: {
        std::string_view s = v.as_string(error);
        ok(error);
        out = cr::value::make(s, error);
        break;
    }
    case cr::value_type::date: {
        cr::date d = v.as_date(error);
        ok(error);
        out = cr::value::make(d, error);
        break;
    }
    case cr::value_type::naive_datetime: {
        cr::naive_datetime d = v.as_naive_datetime(error);
        ok(error);
        out = cr::value::make(d, error);
        break;
    }
    case cr::value_type::aware_datetime: {
        cr::aware_datetime d = v.as_aware_datetime(error);
        ok(error);
        out = cr::value::make(d, error);
        break;
    }
    case cr::value_type::opaque: {
        cr::opaque o = v.as_opaque(error);
        ok(error);
        out = cr::value::make(o, error);
        break;
    }
    case cr::value_type::list:
        out = cr::value::list_of_size(v.size());
        for (std::size_t i = 0; i < v.size(); ++i) {
            out.set(i, rebuild(v[i]), error);
            ok(error);
        }
        break;
    case cr::value_type::map:
        out = v.is_record() ? cr::value::record() : cr::value::map();
        for (std::size_t i = 0; i < v.size(); ++i) {
            auto entry = v.entry(i, error);
            ok(error);
            out.insert(entry.first, rebuild(entry.second), error);
            ok(error);
        }
        break;
    }
    ok(error);
    return out;
}

json::value json_of(cr::value_ref v) {
    cr::error_info error;
    std::string text = v.to_json(error);
    ok(error);
    return json::parse(text);
}

// The input of a vector as native data, which must hold what the library
// reads from its JSON, and survive a copy and a rebuild.
cr::value native_input(const json::value &given, std::string_view key) {
    const json::value &input = need(given, key);
    cr::value out = native(input);
    json::value written = json_of(out);
    cr::error_info error;
    cr::value read = cr::value::from_json(json::write(input), error);
    if (!error && !json::same(written, json_of(read))) {
        throw vector_error("the native input writes " + json::write(written) +
                           ", the library reads " + json::write(json_of(read)));
    }
    cr::value copy = out;
    if (!json::same(json_of(copy), written) || !json::same(json_of(rebuild(out)), written)) {
        throw vector_error("a copy of the input " + json::write(written) + " differs");
    }
    return out;
}

// --- Evaluating ----------------------------------------------------------------

// The output of each rule as the decision document gives it: the values at
// the roots of its payload's trees, or null when the payload was not
// evaluated (spec/json-format.md, *Decisions*).
json::value rule_output(const json::value &result) {
    json::value out;
    const json::value *then = result.get("then");
    if (then == nullptr) {
        return out;
    }
    out.kind = json::type::object;
    for (const auto &member : then->members) {
        out.members.emplace_back(member.first, need(member.second, "value"));
    }
    return out;
}

// Read a decision back with the decision readers, against the expected
// decision document: rules, firing, truth values, selection and outputs
// (spec/rules.md, *Outputs*).
template <class Decision>
std::string check_readers(const Decision &d, const json::value &expected, mode m) {
    const std::vector<json::value> &results = need(expected, "results").items;
    if (d.size() != results.size()) {
        return "size() " + std::to_string(d.size()) + ", expected " +
               std::to_string(results.size());
    }
    std::vector<json::value> outputs;
    for (std::size_t i = 0; i < results.size(); ++i) {
        const json::value &result = results[i];
        const std::string at = "rule " + std::to_string(i) + ": ";
        if (std::string_view name = CALL(m, d.rule_name, i); name != need(result, "rule").text) {
            return at + "rule_name() " + std::string(name);
        }
        if (CALL(m, d.fired, i) != need(result, "fired").boolean) {
            return at + "fired() differs";
        }
        // Graded truth values have no `truth` enumerator.
        const json::value &truth = need(need(result, "when"), "truth");
        if (truth.is(json::type::boolean) || truth.is(json::type::null)) {
            cr::truth want = truth.is(json::type::null) ? cr::truth::unknown
                             : truth.boolean            ? cr::truth::true_
                                                        : cr::truth::false_;
            if (CALL(m, d.truth, i) != want) {
                return at + "truth() differs";
            }
        }
        outputs.push_back(rule_output(result));
        json::value got = json_of(CALL(m, d.rule_output, i));
        if (!json::same(got, outputs.back())) {
            return at + "rule_output() " + json::write(got) + ", expected " +
                   json::write(outputs.back());
        }
    }

    json::value selected;
    selected.kind = json::type::array;
    json::value want_outputs = selected;
    for (std::size_t position : d.selected()) {
        if (position >= results.size()) {
            return "selected() gives the position " + std::to_string(position);
        }
        selected.items.push_back(need(results[position], "rule"));
        want_outputs.items.push_back(outputs[position]);
    }
    if (!json::same(selected, need(expected, "selected"))) {
        return "selected() " + json::write(selected) + ", expected " +
               json::write(need(expected, "selected"));
    }
    json::value got_outputs = json_of(CALL0(m, d.outputs));
    if (!json::same(got_outputs, want_outputs) ||
        !json::same(json::parse(CALL0(m, d.outputs_json)), want_outputs)) {
        return "outputs() " + json::write(got_outputs) + ", expected " +
               json::write(want_outputs);
    }

    // The single output: the aggregate, the winner's payload, or an error for
    // the policies with several.
    const std::string &policy = need(expected, "hit_policy").text;
    json::value want_output;
    if (const json::value *aggregated = expected.get("aggregated")) {
        want_output = *aggregated;
    } else if (policy == "first" || policy == "priority" || policy == "unique" ||
               policy == "any") {
        if (!want_outputs.items.empty()) {
            want_output = want_outputs.items.front();
        }
    } else {
        try {
            json::value got = json_of(CALL0(m, d.output));
            return "output() " + json::write(got) + " under " + policy +
                   ", expected a hit_policy_error";
        } catch (const failure &failed) {
            if (*failed.error.string_at("code") != "hit_policy_error") {
                return "output() failed with " + json::write(failed.error);
            }
        }
        return {};
    }
    json::value got_output = json_of(CALL0(m, d.output));
    if (!json::same(got_output, want_output) ||
        !json::same(json::parse(CALL0(m, d.output_json)), want_output)) {
        return "output() " + json::write(got_output) + ", expected " + json::write(want_output);
    }
    return {};
}

// A condition or a value on its own (spec/ast.md, *Evaluating one node*).
std::string run_node(const json::value &given, const json::value &expect, bool as_value,
                     bool native_run) {
    const mode m = native_run ? mode::info : mode::throwing;
    std::string node_text = json::write(need(given, as_value ? "value" : "condition"));
    cr::node node = CALL(m, cr::node::read, node_text, cr::format::json);
    std::string input_text = json::write(need(given, "input"));
    cr::value input =
        native_run ? native_input(given, "input") : CALL(m, cr::value::from_json, input_text);
    const std::string *logic = given.string_at("logic");
    std::string_view logic_name = logic != nullptr ? std::string_view(*logic) : "boolean";
    const bool strict = given.flag("strict");
    outcome tree = attempt([&] {
        std::string text;
        if (as_value) {
            text = strict ? CALL(m, node.evaluate_value_tree, input, true)
                          : CALL(m, node.evaluate_value_tree, input);
        } else if (logic == nullptr && !strict) {
            text = CALL(m, node.evaluate_tree, input);
        } else {
            text = CALL(m, node.evaluate_tree, input, logic_name, strict);
        }
        return json::parse(text);
    });
    return compare(tree, expect, "result", fields::evaluate);
}

// The explanation of a decision, as JSON and, when the vector has it, as text.
template <class Decision>
std::string explain_decision(const Decision &decision, const json::value &expect, mode m) {
    outcome explanation = attempt(
        [&] { return content(CALL(m, decision.explain, cr::format::json), "explanation"); });
    if (std::string issue = compare(explanation, expect, "explanation", fields::evaluate);
        !issue.empty()) {
        return issue;
    }
    if (const std::string *text = expect.string_at("text")) {
        return compare_text("explained", CALL0(m, decision.explain), *text);
    }
    return {};
}

// An `evaluate` or `explain` vector: one input, or a batch.
std::string run_evaluate(const json::value &given, const json::value &expect, bool explained,
                         bool native_run) {
    const mode m = native_run ? mode::info : mode::throwing;
    cr::ruleset rules = ruleset_of(given, m);

    if (given.get("inputs") != nullptr) {
        std::string inputs_text = json::write(need(given, "inputs"));
        cr::value inputs = native_run ? native_input(given, "inputs")
                                      : CALL(m, cr::value::from_json, inputs_text);
        cr::batch batch;
        outcome decisions = attempt([&] {
            batch = CALL(m, rules.evaluate_many, inputs);
            json::value out;
            out.kind = json::type::array;
            for (cr::decision_ref decision : batch) {
                out.items.push_back(content(CALL0(m, decision.write), "decision"));
            }
            return out;
        });
        if (std::string issue = compare(decisions, expect, "decisions", fields::evaluate);
            !issue.empty() || !native_run || !decisions.result) {
            return issue;
        }
        const std::vector<json::value> &wanted = need(expect, "decisions").items;
        for (std::size_t i = 0; i < wanted.size(); ++i) {
            if (std::string issue = check_readers(batch[i], wanted[i], m); !issue.empty()) {
                return "decision " + std::to_string(i) + ": " + issue;
            }
        }
        return {};
    }

    std::string input_text = json::write(need(given, "input"));
    std::optional<cr::value> input;
    if (native_run) {
        input = native_input(given, "input");
    }
    cr::decision decision;
    outcome written = attempt([&] {
        decision = native_run ? CALL(m, rules.evaluate, *input)
                              : CALL(m, rules.evaluate_json, input_text);
        return content(CALL0(m, decision.write), "decision");
    });
    if (std::string issue = compare(written, expect, "decision", fields::evaluate);
        !issue.empty() || !written.result) {
        return issue;
    }
    if (native_run) {
        if (std::string issue = check_readers(decision, need(expect, "decision"), m);
            !issue.empty()) {
            return issue;
        }
    }
    return explained ? explain_decision(decision, expect, m) : std::string();
}

std::string run_summary(const json::value &given, const json::value &expect, bool native_run) {
    const mode m = native_run ? mode::info : mode::throwing;
    cr::ruleset rules = ruleset_of(given, m);
    outcome summary =
        attempt([&] { return content(CALL(m, rules.summary, cr::format::json), "summary"); });
    if (std::string issue = compare(summary, expect, "summary", fields::evaluate);
        !issue.empty()) {
        return issue;
    }
    if (const std::string *text = expect.string_at("text")) {
        return compare_text("summarised", CALL0(m, rules.summary), *text);
    }
    return {};
}

// --- Running -------------------------------------------------------------------

// The comparison itself, on cases the vectors may not reach: what it gets
// wrong, or nothing.
std::vector<std::string> check_comparison() {
    struct case_ {
        const char *actual;
        const char *expected;
        bool same;
    };
    const case_ cases[] = {
        {"18446744073709551617", "18446744073709551617", true},  // beyond 64 bits
        {"18446744073709551617", "18446744073709551616", false},
        {"-0", "0", true},  // the int 0
        {"1", "1.0", false},
        {"100.0", "1e2", true},
        {"0.1", "0.10000000000000001", true},  // the same double
        {"-0.0", "0.0", false},
        {"[1,2]", "[2,1]", false},
        {R"({"a":1,"b":2})", R"({"b":2,"a":1})", false},
        {R"({"a":1})", R"({"a":1,"b":2})", false},
        {R"({"$opaque":"socket"})", R"({"$opaque":"bytes"})", true},
        {R"({"$opaque":"socket"})", "null", false},
        {R"("é😀")", "\"\xc3\xa9\xf0\x9f\x98\x80\"", true},
        {"true", "1", false},
        {"null", "false", false},
    };
    std::vector<std::string> wrong;
    for (const case_ &c : cases) {
        if (json::same(json::parse(c.actual), json::parse(c.expected)) != c.same) {
            wrong.push_back(std::string("comparison: ") + c.actual + (c.same ? " == " : " != ") +
                            c.expected + " does not hold");
        }
    }
    return wrong;
}

// Run a vector one way: what went wrong, or nothing.
std::string run(const json::value &vector, bool native_run) {
    try {
        const json::value &given = need(vector, "given");
        const json::value &expect = need(vector, "expect");
        const std::string &kind = need(given, "kind").text;
        if (kind == "read") {
            if (std::string issue = run_read(given, expect, mode::throwing); !issue.empty()) {
                return issue;
            }
            if (std::string issue = run_read(given, expect, mode::info); !issue.empty()) {
                return "with error_info: " + issue;
            }
            return {};
        }
        if (kind == "condition" || kind == "value") {
            return run_node(given, expect, kind == "value", native_run);
        }
        if (kind == "evaluate" || kind == "explain") {
            return run_evaluate(given, expect, kind == "explain", native_run);
        }
        if (kind == "summary") {
            return run_summary(given, expect, native_run);
        }
        return "unknown kind " + kind;
    } catch (const failure &failed) {
        return "unexpected error " + json::write(failed.error);
    } catch (const cr::error &e) {
        return "uncaught error " + std::string(e.json());
    } catch (const std::exception &e) {
        return e.what();
    }
}

std::string read_file(const std::filesystem::path &path) {
    std::ifstream in(path, std::ios::binary);
    if (!in) {
        throw std::runtime_error("cannot open " + path.generic_string());
    }
    return std::string(std::istreambuf_iterator<char>(in), std::istreambuf_iterator<char>());
}

// The vector files, in the order of their paths.
std::vector<std::filesystem::path> vector_files(const std::filesystem::path &dir) {
    std::vector<std::filesystem::path> files;
    for (const auto &entry : std::filesystem::recursive_directory_iterator(dir)) {
        if (entry.is_regular_file() && entry.path().extension() == ".json") {
            files.push_back(entry.path());
        }
    }
    std::sort(files.begin(), files.end(), [](const auto &a, const auto &b) {
        return a.generic_string() < b.generic_string();
    });
    return files;
}

}  // namespace

// Usage: cleanrules_conformance [vectors directory]
int main(int argc, char **argv) {
    const std::filesystem::path dir = argc > 1 ? argv[1] : CLEANRULES_VECTORS;
    std::size_t ran = 0;
    std::size_t skipped = 0;
    std::size_t skipped_draw = 0;
    std::size_t passed_json = 0;
    std::size_t ran_native = 0;
    std::size_t passed_native = 0;
    std::vector<std::string> failures = check_comparison();
    try {
        for (const auto &file : vector_files(dir)) {
            json::value document;
            try {
                document = json::parse(read_file(file));
            } catch (const std::exception &e) {
                failures.push_back(file.generic_string() + ": " + e.what());
                continue;
            }
            for (const json::value &vector : need(document, "vectors").items) {
                const std::string &id = need(vector, "id").text;
                // `max_values` bounds native data: C++ builds its values
                // itself, so only the bindings' native runs check these.
                const json::value &given = need(vector, "given");
                // Diagrams are drawn by the core and checked through the C ABI's
                // runner; cleanrules.hpp draws them from phase V.2.
                if (need(given, "kind").text == "draw") {
                    ++skipped_draw;
                    continue;
                }
                if (need(given, "kind").text == "evaluate" && given.get("limits") != nullptr) {
                    ++skipped;
                    continue;
                }
                ++ran;
                if (std::string issue = run(vector, false); issue.empty()) {
                    ++passed_json;
                } else {
                    failures.push_back("json:" + id + ": " + issue);
                }
                if (need(need(vector, "given"), "kind").text == "read") {
                    continue;
                }
                ++ran_native;
                if (std::string issue = run(vector, true); issue.empty()) {
                    ++passed_native;
                } else {
                    failures.push_back("native:" + id + ": " + issue);
                }
            }
        }
    } catch (const std::exception &e) {
        failures.push_back(std::string("cannot read the vectors: ") + e.what());
    }

    for (const std::string &failed : failures) {
        std::fprintf(stderr, "%s\n", failed.c_str());
    }
    std::printf("conformance: %zu vectors; json %zu of %zu passed, native %zu of %zu passed; %zu "
                "native-only skipped; %zu diagrams skipped (spec/visual.md: checked through the "
                "C ABI until cleanrules.hpp draws them)\n",
                ran, passed_json, ran, passed_native, ran_native, skipped, skipped_draw);
    if (!failures.empty()) {
        std::fprintf(stderr, "conformance: %zu failures\n", failures.size());
        return EXIT_FAILURE;
    }
    if (ran < minimum_vectors) {
        std::fprintf(stderr, "conformance: only %zu vectors ran, expected at least %zu\n", ran,
                     minimum_vectors);
        return EXIT_FAILURE;
    }
    return EXIT_SUCCESS;
}
