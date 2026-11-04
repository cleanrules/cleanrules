// A lossless JSON tree for the conformance runner, written for the test so
// that it depends on nothing: object members stay in order, numbers keep the
// text they were written with, and strings decode every escape. Comparing two
// trees follows conformance/README.md (*Comparing*): integers by exact value
// (of any size), floats by exact double value and the sign of zero, and any two
// opaque values as equal. The library's own JSON is never the oracle.

#ifndef CLEANRULES_TESTS_JSON_HPP
#define CLEANRULES_TESTS_JSON_HPP

#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace json {

enum class type { null, boolean, number, string, array, object };

// A JSON value. A number is its text as written; a string, its UTF-8 bytes.
struct value {
    json::type kind = json::type::null;
    bool boolean = false;
    std::string text;
    std::vector<value> items;
    std::vector<std::pair<std::string, value>> members;

    bool is(json::type t) const noexcept { return kind == t; }

    // A number written without a fraction or an exponent.
    bool is_integer() const noexcept {
        return kind == json::type::number && text.find_first_of(".eE") == std::string::npos;
    }

    // The member `key` of an object; null when it has none.
    const value *get(std::string_view key) const noexcept {
        if (kind != json::type::object) {
            return nullptr;
        }
        for (const auto &member : members) {
            if (member.first == key) {
                return &member.second;
            }
        }
        return nullptr;
    }

    // The string member `key`; null when it is missing or not a string.
    const std::string *string_at(std::string_view key) const noexcept {
        const value *found = get(key);
        return found != nullptr && found->is(json::type::string) ? &found->text : nullptr;
    }

    // Whether the member `key` is `true`.
    bool flag(std::string_view key) const noexcept {
        const value *found = get(key);
        return found != nullptr && found->is(json::type::boolean) && found->boolean;
    }
};

class parse_error : public std::runtime_error {
public:
    using std::runtime_error::runtime_error;
};

namespace detail {

class parser {
public:
    explicit parser(std::string_view text) : text_(text) {}

    value document() {
        value out = parse(0);
        skip_space();
        if (pos_ != text_.size()) {
            fail("text after the value");
        }
        return out;
    }

private:
    // Deep enough for every vector (267 levels), shallow enough for the stack.
    static constexpr int max_depth = 2000;

    [[noreturn]] void fail(const char *what) const {
        throw parse_error(std::string("JSON: ") + what + " at byte " + std::to_string(pos_));
    }

    void skip_space() {
        while (pos_ < text_.size() && (text_[pos_] == ' ' || text_[pos_] == '\t' ||
                                       text_[pos_] == '\n' || text_[pos_] == '\r')) {
            ++pos_;
        }
    }

    bool literal(std::string_view word) {
        if (text_.substr(pos_, word.size()) == word) {
            pos_ += word.size();
            return true;
        }
        return false;
    }

    value parse(int depth) {
        if (depth > max_depth) {
            fail("nesting too deep");
        }
        skip_space();
        if (pos_ >= text_.size()) {
            fail("end of text where a value was expected");
        }
        value out;
        char c = text_[pos_];
        if (c == '{') {
            out.kind = type::object;
            ++pos_;
            skip_space();
            if (pos_ < text_.size() && text_[pos_] == '}') {
                ++pos_;
                return out;
            }
            for (;;) {
                skip_space();
                if (pos_ >= text_.size() || text_[pos_] != '"') {
                    fail("an object key must be a string");
                }
                std::string key = string();
                skip_space();
                if (pos_ >= text_.size() || text_[pos_] != ':') {
                    fail("':' expected");
                }
                ++pos_;
                out.members.emplace_back(std::move(key), parse(depth + 1));
                skip_space();
                if (pos_ < text_.size() && text_[pos_] == ',') {
                    ++pos_;
                } else if (pos_ < text_.size() && text_[pos_] == '}') {
                    ++pos_;
                    return out;
                } else {
                    fail("',' or '}' expected");
                }
            }
        }
        if (c == '[') {
            out.kind = type::array;
            ++pos_;
            skip_space();
            if (pos_ < text_.size() && text_[pos_] == ']') {
                ++pos_;
                return out;
            }
            for (;;) {
                out.items.push_back(parse(depth + 1));
                skip_space();
                if (pos_ < text_.size() && text_[pos_] == ',') {
                    ++pos_;
                } else if (pos_ < text_.size() && text_[pos_] == ']') {
                    ++pos_;
                    return out;
                } else {
                    fail("',' or ']' expected");
                }
            }
        }
        if (c == '"') {
            out.kind = type::string;
            out.text = string();
            return out;
        }
        if (literal("true")) {
            out.kind = type::boolean;
            out.boolean = true;
            return out;
        }
        if (literal("false")) {
            out.kind = type::boolean;
            return out;
        }
        if (literal("null")) {
            return out;
        }
        out.kind = type::number;
        out.text = number();
        return out;
    }

    // -?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?, kept as written.
    std::string number() {
        std::size_t start = pos_;
        auto digits = [&] {
            std::size_t first = pos_;
            while (pos_ < text_.size() && text_[pos_] >= '0' && text_[pos_] <= '9') {
                ++pos_;
            }
            return pos_ - first;
        };
        if (pos_ < text_.size() && text_[pos_] == '-') {
            ++pos_;
        }
        std::size_t integer_start = pos_;
        std::size_t integer = digits();
        if (integer == 0) {
            fail("a value was expected");
        }
        if (integer > 1 && text_[integer_start] == '0') {
            fail("a number with a leading zero");
        }
        if (pos_ < text_.size() && text_[pos_] == '.') {
            ++pos_;
            if (digits() == 0) {
                fail("digits expected after '.'");
            }
        }
        if (pos_ < text_.size() && (text_[pos_] == 'e' || text_[pos_] == 'E')) {
            ++pos_;
            if (pos_ < text_.size() && (text_[pos_] == '+' || text_[pos_] == '-')) {
                ++pos_;
            }
            if (digits() == 0) {
                fail("digits expected in the exponent");
            }
        }
        return std::string(text_.substr(start, pos_ - start));
    }

    unsigned hex4() {
        if (pos_ + 4 > text_.size()) {
            fail("a \\u escape needs four hex digits");
        }
        unsigned out = 0;
        for (int i = 0; i < 4; ++i) {
            char c = text_[pos_++];
            out <<= 4;
            if (c >= '0' && c <= '9') {
                out |= static_cast<unsigned>(c - '0');
            } else if (c >= 'a' && c <= 'f') {
                out |= static_cast<unsigned>(c - 'a' + 10);
            } else if (c >= 'A' && c <= 'F') {
                out |= static_cast<unsigned>(c - 'A' + 10);
            } else {
                fail("a \\u escape needs four hex digits");
            }
        }
        return out;
    }

    static void append_utf8(std::string &out, std::uint32_t cp) {
        auto byte = [&](std::uint32_t b) { out.push_back(static_cast<char>(b)); };
        if (cp < 0x80) {
            byte(cp);
        } else if (cp < 0x800) {
            byte(0xC0 | (cp >> 6));
            byte(0x80 | (cp & 0x3F));
        } else if (cp < 0x10000) {
            byte(0xE0 | (cp >> 12));
            byte(0x80 | ((cp >> 6) & 0x3F));
            byte(0x80 | (cp & 0x3F));
        } else {
            byte(0xF0 | (cp >> 18));
            byte(0x80 | ((cp >> 12) & 0x3F));
            byte(0x80 | ((cp >> 6) & 0x3F));
            byte(0x80 | (cp & 0x3F));
        }
    }

    // A string, its escapes decoded (surrogate pairs joined; a lone surrogate
    // has no UTF-8 form and is refused). Raw bytes are copied as they are.
    std::string string() {
        ++pos_;  // the opening quote
        std::string out;
        for (;;) {
            if (pos_ >= text_.size()) {
                fail("a string is not closed");
            }
            char c = text_[pos_++];
            if (c == '"') {
                return out;
            }
            if (static_cast<unsigned char>(c) < 0x20) {
                fail("a control character in a string");
            }
            if (c != '\\') {
                out.push_back(c);
                continue;
            }
            if (pos_ >= text_.size()) {
                fail("a string is not closed");
            }
            char escape = text_[pos_++];
            switch (escape) {
            case '"':
            case '\\':
            case '/':
                out.push_back(escape);
                break;
            case 'b':
                out.push_back('\b');
                break;
            case 'f':
                out.push_back('\f');
                break;
            case 'n':
                out.push_back('\n');
                break;
            case 'r':
                out.push_back('\r');
                break;
            case 't':
                out.push_back('\t');
                break;
            case 'u': {
                std::uint32_t cp = hex4();
                if (cp >= 0xD800 && cp <= 0xDBFF) {
                    if (!literal("\\u")) {
                        fail("a high surrogate without its low one");
                    }
                    std::uint32_t low = hex4();
                    if (low < 0xDC00 || low > 0xDFFF) {
                        fail("a high surrogate without its low one");
                    }
                    cp = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                } else if (cp >= 0xDC00 && cp <= 0xDFFF) {
                    fail("a low surrogate without its high one");
                }
                append_utf8(out, cp);
                break;
            }
            default:
                fail("an unknown escape");
            }
        }
    }

    std::string_view text_;
    std::size_t pos_ = 0;
};

inline void write_string(std::string &out, std::string_view s) {
    static const char hex[] = "0123456789abcdef";
    out.push_back('"');
    for (char c : s) {
        auto byte = static_cast<unsigned char>(c);
        switch (c) {
        case '"':
            out += "\\\"";
            break;
        case '\\':
            out += "\\\\";
            break;
        case '\n':
            out += "\\n";
            break;
        case '\r':
            out += "\\r";
            break;
        case '\t':
            out += "\\t";
            break;
        case '\b':
            out += "\\b";
            break;
        case '\f':
            out += "\\f";
            break;
        default:
            if (byte < 0x20) {
                out += "\\u00";
                out.push_back(hex[byte >> 4]);
                out.push_back(hex[byte & 0xF]);
            } else {
                out.push_back(c);
            }
        }
    }
    out.push_back('"');
}

inline void write(std::string &out, const value &v) {
    switch (v.kind) {
    case type::null:
        out += "null";
        break;
    case type::boolean:
        out += v.boolean ? "true" : "false";
        break;
    case type::number:
        out += v.text;
        break;
    case type::string:
        write_string(out, v.text);
        break;
    case type::array:
        out.push_back('[');
        for (std::size_t i = 0; i < v.items.size(); ++i) {
            if (i > 0) {
                out.push_back(',');
            }
            write(out, v.items[i]);
        }
        out.push_back(']');
        break;
    case type::object:
        out.push_back('{');
        for (std::size_t i = 0; i < v.members.size(); ++i) {
            if (i > 0) {
                out.push_back(',');
            }
            write_string(out, v.members[i].first);
            out.push_back(':');
            write(out, v.members[i].second);
        }
        out.push_back('}');
        break;
    }
}

// An integer's exact value as a sign and its digits; `-0` is `0`.
inline std::pair<bool, std::string_view> integer_parts(std::string_view text) {
    bool negative = !text.empty() && text[0] == '-';
    std::string_view digits = negative ? text.substr(1) : text;
    return {negative && digits != "0", digits};
}

// A float literal's double, rounded to nearest as strtod does in the C locale,
// which this program never changes.
inline double to_double(const std::string &text) { return std::strtod(text.c_str(), nullptr); }

}  // namespace detail

// Parse a JSON text; throws `parse_error`.
inline value parse(std::string_view text) { return detail::parser(text).document(); }

// Compact JSON: numbers as they were written, strings as UTF-8 with only the
// escapes JSON needs.
inline std::string write(const value &v) {
    std::string out;
    detail::write(out, v);
    return out;
}

// A string as a JSON string literal, for messages.
inline std::string quote(std::string_view s) {
    std::string out;
    detail::write_string(out, s);
    return out;
}

// Whether `actual` equals `expected` (conformance/README.md, *Comparing*):
// members in order, numbers by lexical class and exact value, the sign of a
// float zero, and any opaque value for an expected opaque one.
inline bool same(const value &actual, const value &expected) {
    if (expected.get("$opaque") != nullptr) {
        return actual.get("$opaque") != nullptr;
    }
    if (actual.kind != expected.kind) {
        return false;
    }
    switch (expected.kind) {
    case type::null:
        return true;
    case type::boolean:
        return actual.boolean == expected.boolean;
    case type::string:
        return actual.text == expected.text;
    case type::number: {
        if (actual.is_integer() != expected.is_integer()) {
            return false;
        }
        if (expected.is_integer()) {
            return detail::integer_parts(actual.text) == detail::integer_parts(expected.text);
        }
        double a = detail::to_double(actual.text);
        double e = detail::to_double(expected.text);
        return a == e && std::signbit(a) == std::signbit(e);
    }
    case type::array:
        if (actual.items.size() != expected.items.size()) {
            return false;
        }
        for (std::size_t i = 0; i < expected.items.size(); ++i) {
            if (!same(actual.items[i], expected.items[i])) {
                return false;
            }
        }
        return true;
    case type::object:
        if (actual.members.size() != expected.members.size()) {
            return false;
        }
        for (std::size_t i = 0; i < expected.members.size(); ++i) {
            if (actual.members[i].first != expected.members[i].first ||
                !same(actual.members[i].second, expected.members[i].second)) {
                return false;
            }
        }
        return true;
    }
    return false;
}

// The tree without `detail` members, at every level (informative text).
inline value strip_detail(const value &v) {
    value out;
    out.kind = v.kind;
    out.boolean = v.boolean;
    out.text = v.text;
    for (const value &item : v.items) {
        out.items.push_back(strip_detail(item));
    }
    for (const auto &member : v.members) {
        if (member.first != "detail") {
            out.members.emplace_back(member.first, strip_detail(member.second));
        }
    }
    return out;
}

}  // namespace json

#endif  // CLEANRULES_TESTS_JSON_HPP
