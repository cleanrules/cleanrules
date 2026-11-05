// cleanrules for C++17: a header-only layer over the C ABI (cleanrules.h).
//
// Every handle owns its C handle and frees it (RAII); handles move, and
// values also copy. Borrowed views (`value_ref`, `decision_ref`, and the
// `std::string_view`s read from them) point into what they come from and live
// no longer than it; read from a temporary, they are owned copies instead.
//
// Errors, as in std::filesystem: each call that can fail has an overload
// that throws a `cleanrules::error` (one class per code of spec/errors.md)
// and an overload that takes an `error_info &` last and never throws one,
// which it clears on success. Built without exceptions (-fno-exceptions,
// /EHs-c-, or CLEANRULES_NO_EXCEPTIONS defined), only the second exists. The
// two builds live in different inline namespaces, so translation units of
// both kinds can share a program.
//
// Threads: rule sets, documents, decisions, nodes and batches never change
// once made, so they may be used from several threads at once. Values,
// limits, grids and action registries change while they are built: finish
// them first.

#ifndef CLEANRULES_HPP
#define CLEANRULES_HPP

#include <cleanrules.h>

#include <cstddef>
#include <cstdint>
#include <cstdlib>
#include <cstring>
#include <exception>
#include <functional>
#include <initializer_list>
#include <iterator>
#include <limits>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <type_traits>
#include <utility>
#include <vector>

#if !defined(CLEANRULES_NO_EXCEPTIONS) && \
    (defined(__cpp_exceptions) || defined(__EXCEPTIONS) || defined(_CPPUNWIND))
#define CLEANRULES_EXCEPTIONS 1
#include <stdexcept>
#else
#define CLEANRULES_EXCEPTIONS 0
#endif

// The overload of a call that throws: declared only with exceptions.
#if CLEANRULES_EXCEPTIONS
#define CLEANRULES_THROWING(...) __VA_ARGS__
#else
#define CLEANRULES_THROWING(...)
#endif

namespace cleanrules {
#if CLEANRULES_EXCEPTIONS
inline namespace eh {
#else
inline namespace noeh {
#endif

// --- Versions ---------------------------------------------------------------

// The ABI this header was written for; `abi_version()` is the library's.
inline constexpr std::uint32_t header_abi_version = CR_ABI_VERSION;

// The ABI version of the library linked in.
inline std::uint32_t abi_version() noexcept { return cr_abi_version(); }

// The library's version, such as `0.1.0`.
inline std::string_view version() noexcept { return cr_version(); }

// The version of the JSON format the library reads and writes.
inline std::uint32_t format_version() noexcept { return cr_format_version(); }

// --- Enumerations -----------------------------------------------------------
// A name that is a C++ keyword ends in `_`.

// A syntax documents are read from and written in.
enum class format : cr_format {
    json = CR_FORMAT_JSON,
    yaml = CR_FORMAT_YAML,
    text = CR_FORMAT_TEXT,  // the rule syntax; for explanations and summaries, their text
    csv = CR_FORMAT_CSV,    // decision tables
};

// A truth value.
enum class truth : cr_truth {
    true_ = CR_TRUTH_TRUE,
    false_ = CR_TRUTH_FALSE,
    unknown = CR_TRUTH_UNKNOWN,
};

// What a document holds; `none` for an empty handle.
enum class document_content : cr_document_content {
    none = 0,
    condition = CR_DOCUMENT_CONDITION,
    rule = CR_DOCUMENT_RULE,
    ruleset = CR_DOCUMENT_RULESET,
    decision_table = CR_DOCUMENT_DECISION_TABLE,
};

// A limit of `limits`.
enum class limit : cr_limit {
    max_size = CR_LIMIT_MAX_SIZE,  // sets the JSON, YAML and text limits; reads the JSON one
    max_nesting = CR_LIMIT_MAX_NESTING,
    max_depth = CR_LIMIT_MAX_DEPTH,
    max_table_size = CR_LIMIT_MAX_TABLE_SIZE,
    max_yaml_size = CR_LIMIT_MAX_YAML_SIZE,
    max_text_size = CR_LIMIT_MAX_TEXT_SIZE,
};

// What a value is; `none` for an empty handle.
enum class value_type : cr_value_type {
    none = 0,
    null = CR_VALUE_NULL,
    bool_ = CR_VALUE_BOOL,
    int_ = CR_VALUE_INT,
    float_ = CR_VALUE_FLOAT,
    decimal = CR_VALUE_DECIMAL,
    string = CR_VALUE_STRING,
    date = CR_VALUE_DATE,
    naive_datetime = CR_VALUE_NAIVE_DATETIME,
    aware_datetime = CR_VALUE_AWARE_DATETIME,
    list = CR_VALUE_LIST,
    map = CR_VALUE_MAP,  // a map or a record
    opaque = CR_VALUE_OPAQUE,
};

// Why a value is null (spec/data-model.md, *Nulls with a reason*).
enum class null_reason : cr_null_reason {
    null = CR_REASON_NULL,
    int_overflow = CR_REASON_INT_OVERFLOW,
    decimal_overflow = CR_REASON_DECIMAL_OVERFLOW,
    read_error = CR_REASON_READ_ERROR,
};

// The status of a call: `ok`, an error code of spec/errors.md, or a misuse of
// the ABI (`invalid_argument`) or a bug in cleanrules (`internal_error`).
enum class errc : cr_status {
    ok = CR_OK,
    format_error = CR_FORMAT_ERROR,
    unsupported_version = CR_UNSUPPORTED_VERSION,
    limit_exceeded = CR_LIMIT_EXCEEDED,
    decision_mismatch = CR_DECISION_MISMATCH,
    expression_error = CR_EXPRESSION_ERROR,
    rule_definition_error = CR_RULE_DEFINITION_ERROR,
    rule_syntax_error = CR_RULE_SYNTAX_ERROR,
    registry_error = CR_REGISTRY_ERROR,
    rule_not_found = CR_RULE_NOT_FOUND,
    evaluation_error = CR_EVALUATION_ERROR,
    missing_field = CR_MISSING_FIELD,
    type_mismatch = CR_TYPE_MISMATCH,
    hit_policy_error = CR_HIT_POLICY_ERROR,
    action_error = CR_ACTION_ERROR,
    unsupported = CR_UNSUPPORTED,
    invalid_argument = CR_INVALID_ARGUMENT,
    internal_error = CR_INTERNAL_ERROR,
};

// The name of a status (`format_error`, `invalid_argument`, ...); empty for an
// unknown one.
inline std::string_view name(errc code) noexcept {
    const char *text = cr_status_name(static_cast<cr_status>(code));
    return text != nullptr ? std::string_view(text) : std::string_view();
}

// The parent of an error code in the hierarchy of spec/errors.md; `ok` for a
// code without one.
inline errc parent(errc code) noexcept {
    return static_cast<errc>(cr_error_parent(static_cast<cr_status>(code)));
}

// Whether `code` is `ancestor` or one of its descendants.
inline bool is(errc code, errc ancestor) noexcept {
    return cr_error_is(static_cast<cr_status>(code), static_cast<cr_status>(ancestor));
}

class decision;
class error_info;

namespace detail {

template <class T, void (*Free)(T *)>
struct deleter {
    void operator()(T *handle) const noexcept { Free(handle); }
};

template <class T, void (*Free)(T *)>
using owned = std::unique_ptr<T, deleter<T, Free>>;

// A view of borrowed bytes; empty for NULL.
inline std::string_view view(const char *text, std::size_t len) noexcept {
    return text != nullptr ? std::string_view(text, len) : std::string_view();
}

// A text the library gave, copied and freed.
inline std::string take(char *text, std::size_t len) {
    owned<char, cr_string_free> guard(text);
    return text != nullptr ? std::string(text, len) : std::string();
}

inline bool finish(cr_status status, cr_error *raw, error_info *sink,
                   std::exception_ptr cause = nullptr);

}  // namespace detail

// --- Errors -----------------------------------------------------------------

// What went wrong in a call: its code, message, hint and JSON form
// (spec/errors.md, *JSON form*). Empty, and false, when nothing did. Copies
// share the same error; the texts it gives live as long as it, or a copy.
class error_info {
public:
    error_info() noexcept = default;

    // Whether there is an error.
    explicit operator bool() const noexcept { return status_ != CR_OK; }

    errc code() const noexcept { return static_cast<errc>(status_); }

    // Whether the code is `ancestor` or one of its descendants.
    bool is(errc ancestor) const noexcept { return cleanrules::is(code(), ancestor); }

    // The message, written by the library.
    std::string_view message() const noexcept { return text(cr_error_message); }

    // How to fix the problem; empty when there is no hint.
    std::string_view hint() const noexcept { return text(cr_error_hint); }

    // The JSON form: the code, message, hint, context and cause.
    std::string_view json() const noexcept { return text(cr_error_json); }

    // For an `action_error`, a copy of the decision whose action failed, which
    // outlives this error; an empty decision otherwise.
    cleanrules::decision decision() const;

#if CLEANRULES_EXCEPTIONS
    // For an `action_error`, the exception the action threw, if it threw one.
    std::exception_ptr cause() const noexcept { return cause_; }
#endif

    void clear() noexcept { *this = error_info(); }

private:
    friend bool detail::finish(cr_status, cr_error *, error_info *, std::exception_ptr);

    error_info(cr_status status, cr_error *raw, std::exception_ptr cause) noexcept
        : status_(status), error_(raw, cr_error_free), cause_(std::move(cause)) {}

    std::string_view text(const char *(*read)(const cr_error *)) const noexcept {
        const char *text = error_ ? read(error_.get()) : nullptr;
        return text != nullptr ? std::string_view(text) : std::string_view();
    }

    cr_status status_ = CR_OK;
    std::shared_ptr<cr_error> error_;
    std::exception_ptr cause_;
};

#if CLEANRULES_EXCEPTIONS

// The base of every cleanrules exception: `what()` is the message, and
// `info()` the whole error.
class error : public std::runtime_error {
public:
    explicit error(error_info info)
        : std::runtime_error(std::string(info.message())), info_(std::move(info)) {}

    const error_info &info() const noexcept { return info_; }
    errc code() const noexcept { return info_.code(); }
    std::string_view hint() const noexcept { return info_.hint(); }
    std::string_view json() const noexcept { return info_.json(); }

private:
    error_info info_;
};

// A document is invalid: `format_error`, `unsupported_version`,
// `limit_exceeded` and `decision_mismatch`.
class format_error : public error {
public:
    using error::error;
};

// A condition node breaks a construction rule (`expression_error`).
class expression_error : public error {
public:
    using error::error;
};

// A rule or rule set breaks a construction rule (`rule_definition_error`).
class rule_definition_error : public error {
public:
    using error::error;
};

// The rule syntax cannot be parsed (`rule_syntax_error`).
class rule_syntax_error : public error {
public:
    using error::error;
};

// A name is not registered (`registry_error`).
class registry_error : public error {
public:
    using error::error;
};

// A rule set has no rule by that name (`rule_not_found`).
class rule_not_found_error : public error {
public:
    using error::error;
};

// Strict evaluation cannot go on (`evaluation_error` and its children).
class evaluation_error : public error {
public:
    using error::error;
};

// Strict mode met a field that cannot be resolved (`missing_field`).
class missing_field_error : public evaluation_error {
public:
    using evaluation_error::evaluation_error;
};

// Strict mode met values of incompatible types (`type_mismatch`).
class type_mismatch_error : public evaluation_error {
public:
    using evaluation_error::evaluation_error;
};

// The hit policy cannot decide (`hit_policy_error`).
class hit_policy_error : public evaluation_error {
public:
    using evaluation_error::evaluation_error;
};

// An action failed while running a decision (`action_error`). When the action
// threw, this exception is thrown nested over it (std::rethrow_if_nested).
class action_error : public error {
public:
    using error::error;

    // A copy of the decision whose action failed.
    cleanrules::decision decision() const;
};

// This build, logic or format version cannot do the operation (`unsupported`).
class unsupported_error : public error {
public:
    using error::error;
};

// The C ABI was misused (`invalid_argument`): an empty handle, such as a
// moved-from one, an index out of range, an empty action name...
class invalid_argument_error : public error {
public:
    using error::error;
};

// A bug in cleanrules (`internal_error`): please report it.
class internal_error : public error {
public:
    using error::error;
};

// Throw the exception of an error's code.
[[noreturn]] inline void throw_error(error_info info) {
    switch (info.code()) {
    case errc::format_error:
    case errc::unsupported_version:
    case errc::limit_exceeded:
    case errc::decision_mismatch:
        throw format_error(std::move(info));
    case errc::expression_error:
        throw expression_error(std::move(info));
    case errc::rule_definition_error:
        throw rule_definition_error(std::move(info));
    case errc::rule_syntax_error:
        throw rule_syntax_error(std::move(info));
    case errc::registry_error:
        throw registry_error(std::move(info));
    case errc::rule_not_found:
        throw rule_not_found_error(std::move(info));
    case errc::evaluation_error:
        throw evaluation_error(std::move(info));
    case errc::missing_field:
        throw missing_field_error(std::move(info));
    case errc::type_mismatch:
        throw type_mismatch_error(std::move(info));
    case errc::hit_policy_error:
        throw hit_policy_error(std::move(info));
    case errc::action_error:
        if (std::exception_ptr cause = info.cause()) {
            try {
                std::rethrow_exception(cause);
            } catch (...) {
                std::throw_with_nested(action_error(std::move(info)));
            }
        }
        throw action_error(std::move(info));
    case errc::unsupported:
        throw unsupported_error(std::move(info));
    case errc::invalid_argument:
        throw invalid_argument_error(std::move(info));
    case errc::ok:
    case errc::internal_error:
        break;
    }
    throw internal_error(std::move(info));
}

#endif  // CLEANRULES_EXCEPTIONS

namespace detail {

// The end of a call: true on success (clearing `sink`); otherwise the error
// goes to `sink`, or is thrown when there is none.
inline bool finish(cr_status status, cr_error *raw, error_info *sink, std::exception_ptr cause) {
    if (status == CR_OK) {
        if (sink != nullptr) {
            sink->clear();
        }
        return true;
    }
    error_info info(status, raw, std::move(cause));
    if (sink != nullptr) {
        *sink = std::move(info);
        return false;
    }
#if CLEANRULES_EXCEPTIONS
    throw_error(std::move(info));
#else
    // Without exceptions, every call passes a sink: none reaches here.
    std::abort();
#endif
}

// A call into the C ABI with its error out-parameter: `call(&raw)`.
template <class Call>
bool check(error_info *sink, Call &&call) {
    cr_error *raw = nullptr;
    cr_status status = std::forward<Call>(call)(&raw);
    return finish(status, raw, sink);
}

// A text written by the library through `char **` and `size_t *`.
template <class Call>
std::string text(error_info *sink, Call &&call) {
    char *out = nullptr;
    std::size_t len = 0;
    if (!check(sink, [&](cr_error **e) { return std::forward<Call>(call)(&out, &len, e); })) {
        return std::string();
    }
    return take(out, len);
}

}  // namespace detail

// --- Limits -----------------------------------------------------------------

// Limits for reading documents and inputs (spec/json-format.md, *Limits*),
// the defaults at first. They change: set them before sharing them.
class limits {
public:
    limits() : handle_(cr_limits_new()) {}

    CLEANRULES_THROWING(limits &set(limit which, std::size_t value) {
        set_(which, value, nullptr);
        return *this;
    })
    bool set(limit which, std::size_t value, error_info &error) {
        return set_(which, value, &error);
    }

    CLEANRULES_THROWING(std::size_t get(limit which) const { return get_(which, nullptr); })
    std::size_t get(limit which, error_info &error) const { return get_(which, &error); }

    const cr_limits *c_ptr() const noexcept { return handle_.get(); }

private:
    bool set_(limit which, std::size_t value, error_info *sink) {
        return detail::check(sink, [&](cr_error **e) {
            return cr_limits_set(handle_.get(), static_cast<cr_limit>(which), value, e);
        });
    }
    std::size_t get_(limit which, error_info *sink) const {
        std::size_t value = 0;
        detail::check(sink, [&](cr_error **e) {
            return cr_limits_get(handle_.get(), static_cast<cr_limit>(which), &value, e);
        });
        return value;
    }

    detail::owned<cr_limits, cr_limits_free> handle_;
};

namespace detail {
inline const cr_limits *limits_of(const limits *l) noexcept {
    return l != nullptr ? l->c_ptr() : nullptr;
}
}  // namespace detail

// --- Values -----------------------------------------------------------------

// A day, years 1 to 9999.
struct date {
    std::int32_t year = 1;
    std::int32_t month = 1;
    std::int32_t day = 1;
};

// A date and a time of day, to the microsecond, without a zone.
struct naive_datetime {
    std::int32_t year = 1;
    std::int32_t month = 1;
    std::int32_t day = 1;
    std::int32_t hour = 0;
    std::int32_t minute = 0;
    std::int32_t second = 0;
    std::int32_t microsecond = 0;
};

// An instant: seconds since 1970-01-01T00:00:00Z and microseconds after them
// (also before 1970), its UTC offset at that instant, in seconds, and its
// IANA zone's name, empty for a fixed offset.
struct aware_datetime {
    std::int64_t seconds = 0;
    std::int32_t microsecond = 0;
    std::int32_t offset_seconds = 0;
    std::string zone;
};

// A native value with no equivalent: its type's name and a token the program
// chooses, such as an index into its own table.
struct opaque {
    std::string type_name;
    std::uint64_t token = 0;
};

// A null's reason and, for `read_error`, its message.
struct null_info {
    null_reason reason = null_reason::null;
    std::string detail;
};

class value;
class value_ref;

namespace detail {

// The character types: not ints.
template <class T>
inline constexpr bool is_char_v =
    std::is_same_v<T, char> || std::is_same_v<T, signed char> ||
    std::is_same_v<T, unsigned char> || std::is_same_v<T, wchar_t> ||
    std::is_same_v<T, char16_t> || std::is_same_v<T, char32_t>;

// What a value and a borrowed value can both do: read it. What a reader
// borrows (`operator[]`, `entry`, `as_string`) it borrows from an lvalue;
// `value` adds owned versions for temporaries.
template <class Derived>
class value_api {
public:
    // What the value is; `none` for an empty handle.
    value_type kind() const noexcept { return static_cast<value_type>(cr_value_kind(raw())); }
    bool is_null() const noexcept { return kind() == value_type::null; }
    // Whether it is a record: a map whose entries also have positions.
    bool is_record() const noexcept { return cr_value_is_record(raw()); }
    // The number of items of a list or entries of a map; 0 otherwise.
    std::size_t size() const noexcept { return cr_value_len(raw()); }

    // A list's item, or a map's value under a key; empty when there is none.
    value_ref operator[](std::size_t index) const & noexcept;
    value_ref operator[](std::string_view key) const & noexcept;

    // A map's entry by position: its key and its value.
    CLEANRULES_THROWING(std::pair<std::string_view, value_ref> entry(std::size_t index) const &;)
    std::pair<std::string_view, value_ref> entry(std::size_t index, error_info &error) const &;

    CLEANRULES_THROWING(bool as_bool() const { return scalar<bool>(cr_value_get_bool, nullptr); })
    bool as_bool(error_info &error) const { return scalar<bool>(cr_value_get_bool, &error); }

    CLEANRULES_THROWING(std::int64_t as_int() const {
        return scalar<std::int64_t>(cr_value_get_int, nullptr);
    })
    std::int64_t as_int(error_info &error) const {
        return scalar<std::int64_t>(cr_value_get_int, &error);
    }

    CLEANRULES_THROWING(double as_float() const {
        return scalar<double>(cr_value_get_float, nullptr);
    })
    double as_float(error_info &error) const { return scalar<double>(cr_value_get_float, &error); }

    // A decimal's canonical text, such as `0.15`.
    CLEANRULES_THROWING(std::string as_decimal() const { return decimal_(nullptr); })
    std::string as_decimal(error_info &error) const { return decimal_(&error); }

    // A string's UTF-8 bytes, borrowed from the value.
    CLEANRULES_THROWING(std::string_view as_string() const & { return string_(nullptr); })
    std::string_view as_string(error_info &error) const & { return string_(&error); }

    CLEANRULES_THROWING(cleanrules::date as_date() const { return date_(nullptr); })
    cleanrules::date as_date(error_info &error) const { return date_(&error); }

    CLEANRULES_THROWING(cleanrules::naive_datetime as_naive_datetime() const {
        return naive_(nullptr);
    })
    cleanrules::naive_datetime as_naive_datetime(error_info &error) const { return naive_(&error); }

    CLEANRULES_THROWING(cleanrules::aware_datetime as_aware_datetime() const {
        return aware_(nullptr);
    })
    cleanrules::aware_datetime as_aware_datetime(error_info &error) const { return aware_(&error); }

    CLEANRULES_THROWING(cleanrules::opaque as_opaque() const { return opaque_(nullptr); })
    cleanrules::opaque as_opaque(error_info &error) const { return opaque_(&error); }

    CLEANRULES_THROWING(null_info as_null() const { return null_(nullptr); })
    null_info as_null(error_info &error) const { return null_(&error); }

    // The value as JSON (spec/json-format.md, *Values*).
    CLEANRULES_THROWING(std::string to_json() const { return to_json_(nullptr); })
    std::string to_json(error_info &error) const { return to_json_(&error); }

    const cr_value *c_ptr() const noexcept { return raw(); }

protected:
    const cr_value *raw() const noexcept { return static_cast<const Derived &>(*this).raw_(); }

    std::pair<std::string_view, value_ref> entry_(std::size_t index, error_info *sink) const;

    std::string_view string_(error_info *sink) const {
        const char *out = nullptr;
        std::size_t len = 0;
        check(sink, [&](cr_error **e) { return cr_value_get_string(raw(), &out, &len, e); });
        return view(out, len);
    }

private:
    template <class T>
    T scalar(cr_status (*read)(const cr_value *, T *, cr_error **), error_info *sink) const {
        T out{};
        check(sink, [&](cr_error **e) { return read(raw(), &out, e); });
        return out;
    }
    std::string decimal_(error_info *sink) const {
        return text(sink, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_value_get_decimal(raw(), o, n, e);
        });
    }
    cleanrules::date date_(error_info *sink) const {
        cleanrules::date d;
        check(sink, [&](cr_error **e) {
            return cr_value_get_date(raw(), &d.year, &d.month, &d.day, e);
        });
        return d;
    }
    cleanrules::naive_datetime naive_(error_info *sink) const {
        cleanrules::naive_datetime d;
        check(sink, [&](cr_error **e) {
            return cr_value_get_naive_datetime(raw(), &d.year, &d.month, &d.day, &d.hour,
                                               &d.minute, &d.second, &d.microsecond, e);
        });
        return d;
    }
    cleanrules::aware_datetime aware_(error_info *sink) const {
        cleanrules::aware_datetime d;
        const char *zone = nullptr;
        std::size_t len = 0;
        if (check(sink, [&](cr_error **e) {
                return cr_value_get_aware_datetime(raw(), &d.seconds, &d.microsecond,
                                                   &d.offset_seconds, &zone, &len, e);
            })) {
            d.zone = std::string(view(zone, len));
        }
        return d;
    }
    cleanrules::opaque opaque_(error_info *sink) const {
        cleanrules::opaque o;
        const char *name = nullptr;
        std::size_t len = 0;
        if (check(sink, [&](cr_error **e) {
                return cr_value_get_opaque_type(raw(), &name, &len, e);
            }) &&
            check(sink, [&](cr_error **e) {
                return cr_value_get_opaque_token(raw(), &o.token, e);
            })) {
            o.type_name = std::string(view(name, len));
        }
        return o;
    }
    null_info null_(error_info *sink) const {
        null_info n;
        cr_null_reason reason = CR_REASON_NULL;
        const char *detail = nullptr;
        std::size_t len = 0;
        if (check(sink, [&](cr_error **e) {
                return cr_value_get_null(raw(), &reason, &detail, &len, e);
            })) {
            n.reason = static_cast<null_reason>(reason);
            n.detail = std::string(view(detail, len));
        }
        return n;
    }
    std::string to_json_(error_info *sink) const {
        return text(sink, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_value_to_json(raw(), o, n, e);
        });
    }
};

}  // namespace detail

// A value borrowed from another (a list's item, a map's value, a decision's
// input): valid until the value that holds it is changed, moved or freed.
class value_ref : public detail::value_api<value_ref> {
public:
    value_ref() noexcept = default;
    explicit value_ref(const cr_value *raw) noexcept : raw_ptr_(raw) {}

    // Whether it points to a value.
    explicit operator bool() const noexcept { return raw_ptr_ != nullptr; }

    // A copy that owns itself.
    [[nodiscard]] value clone() const;

private:
    friend class detail::value_api<value_ref>;
    const cr_value *raw_() const noexcept { return raw_ptr_; }

    const cr_value *raw_ptr_ = nullptr;
};

// A value of the data model (spec/data-model.md), owned: a null with a
// reason, a boolean, an int, a float, a decimal, a string, a date or
// datetime, a list, a map or record, or an opaque value. Build input with
// the constructors and the factories:
//
//     auto input = cleanrules::value::map({
//         {"customer", cleanrules::value::map({{"years", 7}, {"tier", "gold"}})},
//         {"order", cleanrules::value::map({{"total", 1500.0}})},
//     });
//
// A value that cannot be built (a string that is not UTF-8, an impossible
// date) throws from the constructors; built without exceptions, it is a null
// with the reason `read_error` and the message, as native data that cannot
// be read (spec/data-model.md). The `make` factories report it instead.
class value : public detail::value_api<value> {
public:
    // A plain null.
    value() : handle_(cr_value_new_null()) {}
    value(std::nullptr_t) : value() {}
    value(bool b) : handle_(cr_value_new_bool(b)) {}

    // An int; an unsigned one beyond 64 bits signed is a null with the
    // reason `int_overflow`. Character types are not ints.
    template <class T, std::enable_if_t<std::is_integral_v<T> && !std::is_same_v<T, bool> &&
                                            !detail::is_char_v<T>,
                                        int> = 0>
    value(T i) : handle_(integer(i)) {}

    // A float; NaN is a plain null.
    value(double f) : handle_(cr_value_new_float(f)) {}
    value(float f) : value(static_cast<double>(f)) {}

    // A string, copied (UTF-8); a NULL `const char *` is a plain null. Other
    // pointers are not values.
    value(std::string_view s) : handle_(built(make_string(s))) {}
    value(const char *s) : handle_(s != nullptr ? built(make_string(s)) : cr_value_new_null()) {}
    value(char *s) : value(static_cast<const char *>(s)) {}
    value(const std::string &s) : value(std::string_view(s)) {}
    template <class T>
    value(T *) = delete;

    value(const cleanrules::date &d) : handle_(built(make_date(d))) {}
    value(const cleanrules::naive_datetime &d) : handle_(built(make_naive(d))) {}
    value(const cleanrules::aware_datetime &d) : handle_(built(make_aware(d))) {}
    value(const cleanrules::opaque &o) : handle_(built(make_opaque(o))) {}

    // Values copy deeply, null reasons and opaque tokens included.
    value(const value &other) : handle_(other.clone_raw()) {}
    value &operator=(const value &other) {
        if (this != &other) {
            handle_.reset(other.clone_raw());
        }
        return *this;
    }
    value(value &&) noexcept = default;
    value &operator=(value &&) noexcept = default;
    ~value() = default;

    // Adopt a value from the C ABI, which this one then frees.
    [[nodiscard]] static value adopt(cr_value *raw) noexcept { return value(raw, adopt_tag()); }

    // A borrowed view of this value. A temporary converts too, so that it can
    // be passed to a call, during which it lives.
    value_ref ref() const noexcept { return value_ref(handle_.get()); }
    operator value_ref() const noexcept { return ref(); }

    // Whether it holds a value; false once moved from.
    explicit operator bool() const noexcept { return handle_ != nullptr; }

    // Give the C value to the caller, who frees it with cr_value_free.
    [[nodiscard]] cr_value *release() noexcept { return handle_.release(); }

    // --- Reading a temporary: owned copies, not views into it ---

    using detail::value_api<value>::operator[];
    value operator[](std::size_t index) && { return value_ref(handle_.get())[index].clone(); }
    value operator[](std::string_view key) && { return value_ref(handle_.get())[key].clone(); }

    using detail::value_api<value>::as_string;
    CLEANRULES_THROWING(std::string as_string() && { return std::string(string_(nullptr)); })
    std::string as_string(error_info &error) && { return std::string(string_(&error)); }

    using detail::value_api<value>::entry;
    CLEANRULES_THROWING(std::pair<std::string, value> entry(std::size_t index) && {
        auto e = entry_(index, nullptr);
        return {std::string(e.first), e.second.clone()};
    })
    std::pair<std::string, value> entry(std::size_t index, error_info &error) && {
        auto e = entry_(index, &error);
        return {std::string(e.first), e.second.clone()};
    }

    // --- Factories ---

    // A null with a reason; `detail` is the message of a `read_error`.
    CLEANRULES_THROWING([[nodiscard]] static value null(null_reason reason,
                                                        std::string_view detail = {}) {
        return null_(reason, detail, nullptr);
    })
    [[nodiscard]] static value null(null_reason reason, error_info &error) {
        return null_(reason, {}, &error);
    }
    [[nodiscard]] static value null(null_reason reason, std::string_view detail,
                                    error_info &error) {
        return null_(reason, detail, &error);
    }

    // A decimal from its text (`-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?`, `inf`
    // or `-inf`); one beyond 38 digits is a null with `decimal_overflow`.
    CLEANRULES_THROWING([[nodiscard]] static value decimal(std::string_view text) {
        return decimal_(text, nullptr);
    })
    [[nodiscard]] static value decimal(std::string_view text, error_info &error) {
        return decimal_(text, &error);
    }

    // The checked constructors: an empty value and the error on failure.
    [[nodiscard]] static value make(std::string_view s, error_info &error) {
        return from(make_string(s), &error);
    }
    [[nodiscard]] static value make(const cleanrules::date &d, error_info &error) {
        return from(make_date(d), &error);
    }
    [[nodiscard]] static value make(const cleanrules::naive_datetime &d, error_info &error) {
        return from(make_naive(d), &error);
    }
    [[nodiscard]] static value make(const cleanrules::aware_datetime &d, error_info &error) {
        return from(make_aware(d), &error);
    }
    [[nodiscard]] static value make(const cleanrules::opaque &o, error_info &error) {
        return from(make_opaque(o), &error);
    }

    // A list of these items (empty without them).
    [[nodiscard]] static value list(std::initializer_list<value> items = {}) {
        value out = list_of_size(items.size());
        std::size_t i = 0;
        for (const value &item : items) {
            // Cannot fail: the position exists.
            error_info ignored;
            out.set_(i++, item, &ignored);
        }
        return out;
    }
    // A list of `size` plain nulls, set with `set`.
    [[nodiscard]] static value list_of_size(std::size_t size) {
        return value(cr_value_new_list(size), adopt_tag());
    }

    // An empty map, filled with `insert`, or a map of these entries. A key
    // that is not UTF-8 throws a `format_error`; without exceptions, use the
    // overload that takes an error_info, or `insert`.
    [[nodiscard]] static value map() { return value(cr_value_new_map(), adopt_tag()); }
    CLEANRULES_THROWING([[nodiscard]] static value map(
        std::initializer_list<std::pair<std::string_view, value>> entries) {
        return fill(map(), entries, nullptr);
    })
    [[nodiscard]] static value map(std::initializer_list<std::pair<std::string_view, value>> entries,
                                   error_info &error) {
        return fill(map(), entries, &error);
    }

    // A record: a map whose entries also have positions, their order of
    // insertion (spec/ast.md, *Reading data*).
    [[nodiscard]] static value record() { return value(cr_value_new_record(), adopt_tag()); }
    CLEANRULES_THROWING([[nodiscard]] static value record(
        std::initializer_list<std::pair<std::string_view, value>> entries) {
        return fill(record(), entries, nullptr);
    })
    [[nodiscard]] static value record(
        std::initializer_list<std::pair<std::string_view, value>> entries, error_info &error) {
        return fill(record(), entries, &error);
    }

    // A value read from JSON, as data.
    CLEANRULES_THROWING([[nodiscard]] static value from_json(std::string_view text) {
        return from_json_(text, nullptr, nullptr);
    })
    CLEANRULES_THROWING([[nodiscard]] static value from_json(std::string_view text,
                                                             const limits &l) {
        return from_json_(text, &l, nullptr);
    })
    [[nodiscard]] static value from_json(std::string_view text, error_info &error) {
        return from_json_(text, nullptr, &error);
    }
    [[nodiscard]] static value from_json(std::string_view text, const limits &l,
                                         error_info &error) {
        return from_json_(text, &l, &error);
    }

    // --- Changing lists and maps ---

    // Put `item` at position `index` of this list.
    CLEANRULES_THROWING(value &set(std::size_t index, value item) & {
        set_(index, std::move(item), nullptr);
        return *this;
    })
    CLEANRULES_THROWING(value &&set(std::size_t index, value item) && {
        set_(index, std::move(item), nullptr);
        return std::move(*this);
    })
    bool set(std::size_t index, value item, error_info &error) {
        return set_(index, std::move(item), &error);
    }

    // Put `item` under `key` in this map, replacing a value with that key.
    CLEANRULES_THROWING(value &insert(std::string_view key, value item) & {
        insert_(key, std::move(item), nullptr);
        return *this;
    })
    CLEANRULES_THROWING(value &&insert(std::string_view key, value item) && {
        insert_(key, std::move(item), nullptr);
        return std::move(*this);
    })
    bool insert(std::string_view key, value item, error_info &error) {
        return insert_(key, std::move(item), &error);
    }

private:
    friend class detail::value_api<value>;
    friend class value_ref;
    struct adopt_tag {};

    value(cr_value *raw, adopt_tag) noexcept : handle_(raw) {}
    const cr_value *raw_() const noexcept { return handle_.get(); }

    // A C value that the library made, or the status and error of a failure.
    struct made {
        cr_status status;
        cr_value *out;
        cr_error *error;
    };

    template <class T>
    static cr_value *integer(T i) {
        if constexpr (std::is_unsigned_v<T> && sizeof(T) >= sizeof(std::int64_t)) {
            if (i > static_cast<T>(std::numeric_limits<std::int64_t>::max())) {
                cr_value *out = nullptr;
                cr_value_new_null_reason(CR_REASON_INT_OVERFLOW, nullptr, 0, &out, nullptr);
                return out;
            }
        }
        return cr_value_new_int(static_cast<std::int64_t>(i));
    }

    static made make_string(std::string_view s) {
        made m{CR_OK, nullptr, nullptr};
        m.status = cr_value_new_string(s.data(), s.size(), &m.out, &m.error);
        return m;
    }
    static made make_date(const cleanrules::date &d) {
        made m{CR_OK, nullptr, nullptr};
        m.status = cr_value_new_date(d.year, d.month, d.day, &m.out, &m.error);
        return m;
    }
    static made make_naive(const cleanrules::naive_datetime &d) {
        made m{CR_OK, nullptr, nullptr};
        m.status = cr_value_new_naive_datetime(d.year, d.month, d.day, d.hour, d.minute, d.second,
                                               d.microsecond, &m.out, &m.error);
        return m;
    }
    static made make_aware(const cleanrules::aware_datetime &d) {
        made m{CR_OK, nullptr, nullptr};
        // No zone, a fixed offset: NULL with a length of 0.
        const char *zone = d.zone.empty() ? nullptr : d.zone.data();
        m.status = cr_value_new_aware_datetime(d.seconds, d.microsecond, d.offset_seconds, zone,
                                               d.zone.size(), &m.out, &m.error);
        return m;
    }
    static made make_opaque(const cleanrules::opaque &o) {
        made m{CR_OK, nullptr, nullptr};
        m.status =
            cr_value_new_opaque(o.type_name.data(), o.type_name.size(), o.token, &m.out, &m.error);
        return m;
    }

    // A constructor's value: it throws on failure, or, without exceptions,
    // gives a null that keeps the message.
    static cr_value *built(made m) {
        if (m.status == CR_OK) {
            return m.out;
        }
#if CLEANRULES_EXCEPTIONS
        detail::finish(m.status, m.error, nullptr);
#endif
        const char *message = cr_error_message(m.error);
        cr_value *null = nullptr;
        cr_value_new_null_reason(CR_REASON_READ_ERROR, message,
                                 message != nullptr ? std::strlen(message) : 0, &null, nullptr);
        cr_error_free(m.error);
        return null;
    }

    static value from(made m, error_info *sink) {
        if (!detail::finish(m.status, m.error, sink)) {
            return value(nullptr, adopt_tag());
        }
        return value(m.out, adopt_tag());
    }

    static value null_(null_reason reason, std::string_view detail, error_info *sink) {
        made m{CR_OK, nullptr, nullptr};
        m.status = cr_value_new_null_reason(static_cast<cr_null_reason>(reason), detail.data(),
                                            detail.size(), &m.out, &m.error);
        return from(m, sink);
    }
    static value decimal_(std::string_view text, error_info *sink) {
        made m{CR_OK, nullptr, nullptr};
        m.status = cr_value_new_decimal(text.data(), text.size(), &m.out, &m.error);
        return from(m, sink);
    }
    static value from_json_(std::string_view text, const limits *l, error_info *sink) {
        made m{CR_OK, nullptr, nullptr};
        m.status =
            cr_value_from_json(text.data(), text.size(), detail::limits_of(l), &m.out, &m.error);
        return from(m, sink);
    }

    // The entries in order; on the first that fails, an empty value.
    static value fill(value out, std::initializer_list<std::pair<std::string_view, value>> entries,
                      error_info *sink) {
        for (const auto &entry : entries) {
            if (!out.insert_(entry.first, entry.second, sink)) {
                return value(nullptr, adopt_tag());
            }
        }
        return out;
    }

    cr_value *clone_raw() const {
        cr_value *out = nullptr;
        if (handle_ != nullptr) {
            cr_value_clone(handle_.get(), &out, nullptr);
        }
        return out;
    }

    // The item moves into the C value, which consumes it even on failure.
    bool set_(std::size_t index, value item, error_info *sink) {
        return detail::check(sink, [&](cr_error **e) {
            return cr_value_list_set(handle_.get(), index, item.release(), e);
        });
    }
    bool insert_(std::string_view key, value item, error_info *sink) {
        return detail::check(sink, [&](cr_error **e) {
            return cr_value_map_insert(handle_.get(), key.data(), key.size(), item.release(), e);
        });
    }

    detail::owned<cr_value, cr_value_free> handle_;
};

inline value value_ref::clone() const {
    cr_value *out = nullptr;
    if (raw_ptr_ != nullptr) {
        cr_value_clone(raw_ptr_, &out, nullptr);
    }
    return value::adopt(out);
}

namespace detail {

template <class Derived>
value_ref value_api<Derived>::operator[](std::size_t index) const & noexcept {
    return value_ref(cr_value_list_get(raw(), index));
}

template <class Derived>
value_ref value_api<Derived>::operator[](std::string_view key) const & noexcept {
    return value_ref(cr_value_map_get(raw(), key.data(), key.size()));
}

template <class Derived>
std::pair<std::string_view, value_ref> value_api<Derived>::entry_(std::size_t index,
                                                                  error_info *sink) const {
    const char *key = nullptr;
    std::size_t len = 0;
    const cr_value *item = nullptr;
    check(sink,
          [&](cr_error **e) { return cr_value_map_entry(raw(), index, &key, &len, &item, e); });
    return {view(key, len), value_ref(item)};
}

#if CLEANRULES_EXCEPTIONS
template <class Derived>
std::pair<std::string_view, value_ref> value_api<Derived>::entry(std::size_t index) const & {
    return entry_(index, nullptr);
}
#endif

template <class Derived>
std::pair<std::string_view, value_ref> value_api<Derived>::entry(std::size_t index,
                                                                 error_info &error) const & {
    return entry_(index, &error);
}

}  // namespace detail

// --- Decisions --------------------------------------------------------------

namespace detail {

// What a decision and a borrowed decision can both do.
template <class Derived>
class decision_api {
public:
    // The number of rules of the decision's rule set.
    std::size_t size() const noexcept { return cr_decision_len(raw()); }

    // Whether the rule at `rule` fired.
    CLEANRULES_THROWING(bool fired(std::size_t rule) const { return fired_(rule, nullptr); })
    bool fired(std::size_t rule, error_info &error) const { return fired_(rule, &error); }

    // The truth value of the rule's condition.
    CLEANRULES_THROWING(cleanrules::truth truth(std::size_t rule) const {
        return truth_(rule, nullptr);
    })
    cleanrules::truth truth(std::size_t rule, error_info &error) const {
        return truth_(rule, &error);
    }

    // The positions of the rules the hit policy selected, in rule order.
    std::vector<std::size_t> selected() const {
        std::vector<std::size_t> out(cr_decision_selected_len(raw()));
        for (std::size_t i = 0; i < out.size(); ++i) {
            cr_decision_selected(raw(), i, &out[i], nullptr);
        }
        return out;
    }

    // The name of the rule at `rule`, borrowed from the decision.
    CLEANRULES_THROWING(std::string_view rule_name(std::size_t rule) const & {
        return rule_name_(rule, nullptr);
    })
    std::string_view rule_name(std::size_t rule, error_info &error) const & {
        return rule_name_(rule, &error);
    }

    // The output of the rule at `rule`: a map of its payload when it was
    // evaluated, a plain null otherwise.
    CLEANRULES_THROWING(value rule_output(std::size_t rule) const {
        return value_of(nullptr, [&](cr_value **o, cr_error **e) {
            return cr_decision_rule_output(raw(), rule, o, e);
        });
    })
    value rule_output(std::size_t rule, error_info &error) const {
        return value_of(&error, [&](cr_value **o, cr_error **e) {
            return cr_decision_rule_output(raw(), rule, o, e);
        });
    }

    // The single output (spec/rules.md, *Outputs*): the winner's payload as a
    // map, the aggregate, or a plain null when no rule was selected. Policies
    // with several outputs throw `hit_policy_error`: use `outputs`.
    CLEANRULES_THROWING(value output() const {
        return value_of(nullptr, [&](cr_value **o, cr_error **e) {
            return cr_decision_output_value(raw(), o, e);
        });
    })
    value output(error_info &error) const {
        return value_of(&error, [&](cr_value **o, cr_error **e) {
            return cr_decision_output_value(raw(), o, e);
        });
    }

    // The outputs of the selected rules, in rule order: a list of maps.
    CLEANRULES_THROWING(value outputs() const {
        return value_of(nullptr, [&](cr_value **o, cr_error **e) {
            return cr_decision_outputs_value(raw(), o, e);
        });
    })
    value outputs(error_info &error) const {
        return value_of(&error, [&](cr_value **o, cr_error **e) {
            return cr_decision_outputs_value(raw(), o, e);
        });
    }

    // The same, as JSON.
    CLEANRULES_THROWING(std::string output_json() const {
        return text(nullptr, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_decision_output(raw(), o, n, e);
        });
    })
    std::string output_json(error_info &error) const {
        return text(&error, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_decision_output(raw(), o, n, e);
        });
    }
    CLEANRULES_THROWING(std::string outputs_json() const {
        return text(nullptr, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_decision_outputs(raw(), o, n, e);
        });
    })
    std::string outputs_json(error_info &error) const {
        return text(&error, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_decision_outputs(raw(), o, n, e);
        });
    }

    // The decision as a JSON (by default) or YAML document.
    CLEANRULES_THROWING(std::string write(format f = format::json) const {
        return write_(f, nullptr);
    })
    std::string write(error_info &error) const { return write_(format::json, &error); }
    std::string write(format f, error_info &error) const { return write_(f, &error); }

    // The explanation (spec/explain.md): text by default, or a JSON or YAML
    // document.
    CLEANRULES_THROWING(std::string explain(format f = format::text) const {
        return explain_(f, nullptr);
    })
    std::string explain(error_info &error) const { return explain_(format::text, &error); }
    std::string explain(format f, error_info &error) const { return explain_(f, &error); }

    // The explanation as a value, to build objects of it: the explanation
    // document's structure with the values themselves, and the `text` of each
    // rule and cause and whether each cause `holds` (`cr_decision_explain_value`).
    CLEANRULES_THROWING([[nodiscard]] value explanation() const { return explanation_(nullptr); })
    [[nodiscard]] value explanation(error_info &error) const { return explanation_(&error); }

    // A copy that owns itself, to keep beyond what lends this one.
    [[nodiscard]] cleanrules::decision clone() const;

    const cr_decision *c_ptr() const noexcept { return raw(); }

protected:
    const cr_decision *raw() const noexcept {
        return static_cast<const Derived &>(*this).raw_();
    }

    std::string_view rule_name_(std::size_t rule, error_info *sink) const {
        const char *name = nullptr;
        std::size_t len = 0;
        check(sink,
              [&](cr_error **e) { return cr_decision_rule_name(raw(), rule, &name, &len, e); });
        return view(name, len);
    }

private:
    bool fired_(std::size_t rule, error_info *sink) const {
        bool out = false;
        check(sink, [&](cr_error **e) { return cr_decision_fired(raw(), rule, &out, e); });
        return out;
    }
    cleanrules::truth truth_(std::size_t rule, error_info *sink) const {
        cr_truth out = CR_TRUTH_UNKNOWN;
        check(sink, [&](cr_error **e) { return cr_decision_truth(raw(), rule, &out, e); });
        return static_cast<cleanrules::truth>(out);
    }
    template <class Call>
    static value value_of(error_info *sink, Call &&call) {
        cr_value *out = nullptr;
        check(sink, [&](cr_error **e) { return call(&out, e); });
        return value::adopt(out);
    }
    std::string write_(format f, error_info *sink) const {
        return text(sink, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_decision_write(raw(), static_cast<cr_format>(f), o, n, e);
        });
    }
    std::string explain_(format f, error_info *sink) const {
        return text(sink, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_decision_explain(raw(), static_cast<cr_format>(f), o, n, e);
        });
    }
    value explanation_(error_info *sink) const {
        cr_value *out = nullptr;
        check(sink, [&](cr_error **e) { return cr_decision_explain_value(raw(), &out, e); });
        return value::adopt(out);
    }
};

}  // namespace detail

// A decision borrowed from a batch or an action call: valid as long as what
// it comes from; `clone()` keeps it longer.
class decision_ref : public detail::decision_api<decision_ref> {
public:
    decision_ref() noexcept = default;
    explicit decision_ref(const cr_decision *raw) noexcept : raw_ptr_(raw) {}

    explicit operator bool() const noexcept { return raw_ptr_ != nullptr; }

private:
    friend class detail::decision_api<decision_ref>;
    const cr_decision *raw_() const noexcept { return raw_ptr_; }

    const cr_decision *raw_ptr_ = nullptr;
};

// A decision: what a rule set made of one input, or a recorded decision read
// with its rule set. It keeps its rule set alive, and a copy of its input.
class decision : public detail::decision_api<decision> {
public:
    decision() noexcept = default;

    // Adopt a decision from the C ABI, which this one then frees.
    [[nodiscard]] static decision adopt(cr_decision *raw) noexcept {
        decision d;
        d.handle_.reset(raw);
        return d;
    }

    explicit operator bool() const noexcept { return handle_ != nullptr; }
    decision_ref ref() const noexcept { return decision_ref(handle_.get()); }
    operator decision_ref() const noexcept { return ref(); }

    // The rule's name, owned when read from a temporary decision.
    using detail::decision_api<decision>::rule_name;
    CLEANRULES_THROWING(std::string rule_name(std::size_t rule) && {
        return std::string(rule_name_(rule, nullptr));
    })
    std::string rule_name(std::size_t rule, error_info &error) && {
        return std::string(rule_name_(rule, &error));
    }

private:
    friend class detail::decision_api<decision>;
    const cr_decision *raw_() const noexcept { return handle_.get(); }

    detail::owned<cr_decision, cr_decision_free> handle_;
};

namespace detail {
template <class Derived>
cleanrules::decision decision_api<Derived>::clone() const {
    cr_decision *out = nullptr;
    if (raw() != nullptr) {
        cr_decision_clone(raw(), &out, nullptr);
    }
    return cleanrules::decision::adopt(out);
}
}  // namespace detail

inline decision error_info::decision() const {
    return decision_ref(error_ ? cr_error_decision(error_.get()) : nullptr).clone();
}

#if CLEANRULES_EXCEPTIONS
inline decision action_error::decision() const { return info().decision(); }
#endif

// The decisions of `ruleset::evaluate_many`, one per input, in order.
class batch {
public:
    batch() noexcept = default;

    std::size_t size() const noexcept { return cr_batch_len(handle_.get()); }
    bool empty() const noexcept { return size() == 0; }

    // The decision of the input at `index`, borrowed from the batch; empty
    // when `index` is out of it.
    decision_ref operator[](std::size_t index) const noexcept {
        return decision_ref(cr_batch_get(handle_.get(), index));
    }

    class iterator {
    public:
        using iterator_category = std::input_iterator_tag;
        using value_type = decision_ref;
        using difference_type = std::ptrdiff_t;
        using pointer = void;
        using reference = decision_ref;

        iterator() noexcept = default;
        decision_ref operator*() const noexcept { return (*batch_)[index_]; }
        iterator &operator++() noexcept {
            ++index_;
            return *this;
        }
        iterator operator++(int) noexcept {
            iterator old = *this;
            ++index_;
            return old;
        }
        bool operator==(const iterator &other) const noexcept { return index_ == other.index_; }
        bool operator!=(const iterator &other) const noexcept { return index_ != other.index_; }

    private:
        friend class batch;
        iterator(const batch *b, std::size_t i) noexcept : batch_(b), index_(i) {}
        const batch *batch_ = nullptr;
        std::size_t index_ = 0;
    };

    iterator begin() const noexcept { return iterator(this, 0); }
    iterator end() const noexcept { return iterator(this, size()); }

    explicit operator bool() const noexcept { return handle_ != nullptr; }
    const cr_batch *c_ptr() const noexcept { return handle_.get(); }

private:
    friend class ruleset;
    detail::owned<cr_batch, cr_batch_free> handle_;
};

// --- Actions ----------------------------------------------------------------

// One call of an action: the decision, the rule and the input, valid during
// the call only.
class action_call {
public:
    explicit action_call(cr_action_call *raw) noexcept : raw_(raw) {}

    // The decision being run (`clone()` keeps it after the call).
    decision_ref decision() const noexcept { return decision_ref(cr_action_call_decision(raw_)); }
    // The position of the rule whose action is called.
    std::size_t rule() const noexcept { return cr_action_call_rule(raw_); }
    // Its name, borrowed from the decision.
    std::string_view rule_name() const noexcept {
        const char *name = nullptr;
        std::size_t len = 0;
        cr_decision_rule_name(cr_action_call_decision(raw_), rule(), &name, &len, nullptr);
        return detail::view(name, len);
    }
    // The input, as the decision keeps it.
    value_ref input() const noexcept { return value_ref(cr_action_call_input(raw_)); }

    // Fail the action, saying why: the run stops with an `action_error` whose
    // cause is `reason`, once the action returns.
    void fail(std::string_view reason) noexcept {
        cr_action_call_fail(raw_, reason.data(), reason.size());
        failed_ = true;
    }
    bool failed() const noexcept { return failed_; }

private:
    cr_action_call *raw_;
    bool failed_ = false;
};

// An action: called by `ruleset::run` for each selected rule that names it. It
// fails by calling `fail` or by throwing, which stops the run.
using action = std::function<void(action_call &)>;

namespace detail {

// The exception an action threw during the innermost `ruleset::run` of this
// thread, which actions run on. On Windows, each DLL or program that includes
// this header has its own: an exception thrown by an action registered in one
// module, during a run started in another, still fails the run, but is not
// nested in its `action_error`.
class run_frame {
public:
    run_frame() noexcept : outer_(top()) { top() = this; }
    ~run_frame() { top() = outer_; }
    run_frame(const run_frame &) = delete;
    run_frame &operator=(const run_frame &) = delete;

    static void store(std::exception_ptr thrown) noexcept {
        if (run_frame *frame = top()) {
            frame->thrown_ = std::move(thrown);
        }
    }
    std::exception_ptr thrown() const noexcept { return thrown_; }

private:
    static run_frame *&top() noexcept {
        static thread_local run_frame *frame = nullptr;
        return frame;
    }

    run_frame *outer_;
    std::exception_ptr thrown_;
};

// The C action of every C++ action: no exception crosses the C frames.
inline std::int32_t trampoline(void *data, cr_action_call *raw) noexcept {
    action_call call(raw);
#if CLEANRULES_EXCEPTIONS
    try {
        (*static_cast<action *>(data))(call);
    } catch (const std::exception &thrown) {
        call.fail(thrown.what());
        run_frame::store(std::current_exception());
        return 1;
    } catch (...) {
        call.fail("the action threw an exception that is not a std::exception");
        run_frame::store(std::current_exception());
        return 1;
    }
#else
    (*static_cast<action *>(data))(call);
#endif
    return call.failed() ? 1 : 0;
}

}  // namespace detail

// An action registry (spec/rules.md, *Actions*): names mapped to actions,
// which it keeps. Register them all before running; a finished registry may
// be used from several threads, whose actions then run concurrently.
class actions {
public:
    actions() : handle_(cr_actions_new()) {}

    // Register `fn` under `name`: not empty, and registered once.
    CLEANRULES_THROWING(actions &add(std::string_view name, action fn) {
        add_(name, std::move(fn), nullptr);
        return *this;
    })
    bool add(std::string_view name, action fn, error_info &error) {
        return add_(name, std::move(fn), &error);
    }

    const cr_actions *c_ptr() const noexcept { return handle_.get(); }

private:
    bool add_(std::string_view name, action fn, error_info *sink) {
        auto held = std::make_unique<action>(std::move(fn));
        // Room first: once registered, keeping the action cannot fail.
        fns_.reserve(fns_.size() + 1);
        bool added = detail::check(sink, [&](cr_error **e) {
            return cr_actions_add(handle_.get(), name.data(), name.size(), detail::trampoline,
                                  held.get(), e);
        });
        if (added) {
            fns_.push_back(std::move(held));
        }
        return added;
    }

    // Declared first so that the registry is freed before what it points to.
    std::vector<std::unique_ptr<action>> fns_;
    detail::owned<cr_actions, cr_actions_free> handle_;
};

// --- Documents, rule sets, nodes and grids ----------------------------------

// A document read in any format: a condition, a rule, a rule set or a
// decision table.
class document {
public:
    document() noexcept = default;

    // Read a document: JSON, YAML, the rule syntax, or CSV for a decision
    // table. Bytes that are not UTF-8 are a `format_error`.
    CLEANRULES_THROWING([[nodiscard]] static document read(std::string_view text, format f) {
        return read_(text, f, nullptr, nullptr);
    })
    CLEANRULES_THROWING([[nodiscard]] static document read(std::string_view text, format f,
                                                           const limits &l) {
        return read_(text, f, &l, nullptr);
    })
    [[nodiscard]] static document read(std::string_view text, format f, error_info &error) {
        return read_(text, f, nullptr, &error);
    }
    [[nodiscard]] static document read(std::string_view text, format f, const limits &l,
                                       error_info &error) {
        return read_(text, f, &l, &error);
    }

    // What it holds.
    document_content content() const noexcept {
        return static_cast<document_content>(cr_document_kind(handle_.get()));
    }

    // Write it in canonical form: JSON by default.
    CLEANRULES_THROWING(std::string write(format f = format::json) const {
        return write_(f, nullptr);
    })
    std::string write(error_info &error) const { return write_(format::json, &error); }
    std::string write(format f, error_info &error) const { return write_(f, &error); }

    explicit operator bool() const noexcept { return handle_ != nullptr; }
    const cr_document *c_ptr() const noexcept { return handle_.get(); }

private:
    friend class grid;

    static document read_(std::string_view text, format f, const limits *l, error_info *sink) {
        document d;
        cr_document *out = nullptr;
        detail::check(sink, [&](cr_error **e) {
            return cr_document_read(text.data(), text.size(), static_cast<cr_format>(f),
                                    detail::limits_of(l), &out, e);
        });
        d.handle_.reset(out);
        return d;
    }
    std::string write_(format f, error_info *sink) const {
        return detail::text(sink, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_document_write(handle_.get(), static_cast<cr_format>(f), o, n, e);
        });
    }

    detail::owned<cr_document, cr_document_free> handle_;
};

class rule;
struct table_settings;

// A rule passed to `ruleset::make`: a named rule or a temporary, which lives
// until the call returns.
class rule_ref {
public:
    rule_ref(const rule &r) noexcept : ptr_(&r) {}
    const rule &get() const noexcept { return *ptr_; }

private:
    const rule *ptr_;
};

// Rules passed to `ruleset::make`, in order: `{vip, loyal}`.
using rule_refs = std::vector<rule_ref>;

// A rule set, ready to evaluate.
class ruleset {
public:
    ruleset() noexcept = default;

    // A rule set of rules built in code, in order (copied), with the hit
    // policy, aggregate and logic of `settings` (empty: `first`, none and
    // `boolean`); defined after `rule`.
    CLEANRULES_THROWING([[nodiscard]] static ruleset make(const rule_refs &rules);)
    CLEANRULES_THROWING([[nodiscard]] static ruleset make(const rule_refs &rules,
                                                          const table_settings &settings);)
    [[nodiscard]] static ruleset make(const rule_refs &rules, error_info &error);
    [[nodiscard]] static ruleset make(const rule_refs &rules, const table_settings &settings,
                                      error_info &error);

    // The position of the rule named `name`; a `rule_not_found_error`, with
    // the names there are, when there is none.
    CLEANRULES_THROWING(std::size_t rule_position(std::string_view name) const {
        return rule_position_(name, nullptr);
    })
    std::size_t rule_position(std::string_view name, error_info &error) const {
        return rule_position_(name, &error);
    }

    // Read a rule set: a rule set, a decision table (compiled), or one rule.
    CLEANRULES_THROWING([[nodiscard]] static ruleset read(std::string_view text, format f) {
        return read_(text, f, nullptr, nullptr);
    })
    CLEANRULES_THROWING([[nodiscard]] static ruleset read(std::string_view text, format f,
                                                          const limits &l) {
        return read_(text, f, &l, nullptr);
    })
    [[nodiscard]] static ruleset read(std::string_view text, format f, error_info &error) {
        return read_(text, f, nullptr, &error);
    }
    [[nodiscard]] static ruleset read(std::string_view text, format f, const limits &l,
                                      error_info &error) {
        return read_(text, f, &l, &error);
    }

    // The rule set a document gives; a condition is a `format_error`.
    CLEANRULES_THROWING([[nodiscard]] static ruleset from_document(const document &d) {
        return from_document_(d, nullptr);
    })
    [[nodiscard]] static ruleset from_document(const document &d, error_info &error) {
        return from_document_(d, &error);
    }

    // The number of rules.
    std::size_t size() const noexcept { return cr_ruleset_len(raw()); }

    // The name of the rule at `rule`, borrowed from the rule set (owned when
    // read from a temporary one).
    CLEANRULES_THROWING(std::string_view rule_name(std::size_t rule) const & {
        return rule_name_(rule, nullptr);
    })
    std::string_view rule_name(std::size_t rule, error_info &error) const & {
        return rule_name_(rule, &error);
    }
    CLEANRULES_THROWING(std::string rule_name(std::size_t rule) && {
        return std::string(rule_name_(rule, nullptr));
    })
    std::string rule_name(std::size_t rule, error_info &error) && {
        return std::string(rule_name_(rule, &error));
    }

    // The hit policy (`first`, `priority`, `unique`, `any`, `rule_order`,
    // `collect`).
    std::string_view hit_policy() const noexcept {
        const char *name = cr_ruleset_hit_policy(raw());
        return name != nullptr ? std::string_view(name) : std::string_view();
    }
    // The logic (`boolean`).
    std::string_view logic() const noexcept {
        const char *name = cr_ruleset_logic(raw());
        return name != nullptr ? std::string_view(name) : std::string_view();
    }
    // The aggregate (`sum`, `min`, `max`, `count`), if it has one.
    std::optional<std::string_view> aggregate() const noexcept {
        const char *name = cr_ruleset_aggregate(raw());
        return name != nullptr ? std::optional<std::string_view>(name) : std::nullopt;
    }
    bool strict() const noexcept { return cr_ruleset_strict(raw()); }

    // Write it: JSON by default, YAML or the rule syntax.
    CLEANRULES_THROWING(std::string write(format f = format::json) const {
        return write_(f, nullptr);
    })
    std::string write(error_info &error) const { return write_(format::json, &error); }
    std::string write(format f, error_info &error) const { return write_(f, &error); }

    // The summary (spec/explain.md, *Summaries*): text by default, or a JSON
    // or YAML document.
    CLEANRULES_THROWING(std::string summary(format f = format::text) const {
        return summary_(f, nullptr);
    })
    std::string summary(error_info &error) const { return summary_(format::text, &error); }
    std::string summary(format f, error_info &error) const { return summary_(f, &error); }

    // The summary with the rules whose action is code of the program marked:
    // one flag per rule, in rule order.
    CLEANRULES_THROWING(std::string summary(const std::vector<bool> &actions,
                                            format f = format::text) const {
        return summary_actions_(actions, f, nullptr);
    })
    std::string summary(const std::vector<bool> &actions, format f, error_info &error) const {
        return summary_actions_(actions, f, &error);
    }

    // The paths of the input that the rule set reads: a list of paths, each a
    // list of strings (keys) and ints (indexes).
    CLEANRULES_THROWING([[nodiscard]] value field_paths() const { return field_paths_(nullptr); })
    [[nodiscard]] value field_paths(error_info &error) const { return field_paths_(&error); }

    // Evaluate on an input; the decision keeps a copy of it.
    CLEANRULES_THROWING([[nodiscard]] decision evaluate(value_ref input) const {
        return evaluate_(input, nullptr, nullptr);
    })
    CLEANRULES_THROWING([[nodiscard]] decision evaluate(value_ref input, const limits &l) const {
        return evaluate_(input, &l, nullptr);
    })
    [[nodiscard]] decision evaluate(value_ref input, error_info &error) const {
        return evaluate_(input, nullptr, &error);
    }
    [[nodiscard]] decision evaluate(value_ref input, const limits &l, error_info &error) const {
        return evaluate_(input, &l, &error);
    }

    // Evaluate on an input given as JSON, such as `{"customer": {"years": 7}}`.
    CLEANRULES_THROWING([[nodiscard]] decision evaluate_json(std::string_view input) const {
        return evaluate_json_(input, nullptr, nullptr);
    })
    CLEANRULES_THROWING([[nodiscard]] decision evaluate_json(std::string_view input,
                                                             const limits &l) const {
        return evaluate_json_(input, &l, nullptr);
    })
    [[nodiscard]] decision evaluate_json(std::string_view input, error_info &error) const {
        return evaluate_json_(input, nullptr, &error);
    }
    [[nodiscard]] decision evaluate_json(std::string_view input, const limits &l,
                                         error_info &error) const {
        return evaluate_json_(input, &l, &error);
    }

    // Evaluate on each input of a list; the first that fails gives the error,
    // with its position as `row`.
    CLEANRULES_THROWING([[nodiscard]] batch evaluate_many(value_ref inputs) const {
        return evaluate_many_(inputs, nullptr, nullptr);
    })
    CLEANRULES_THROWING([[nodiscard]] batch evaluate_many(value_ref inputs,
                                                          const limits &l) const {
        return evaluate_many_(inputs, &l, nullptr);
    })
    [[nodiscard]] batch evaluate_many(value_ref inputs, error_info &error) const {
        return evaluate_many_(inputs, nullptr, &error);
    }
    [[nodiscard]] batch evaluate_many(value_ref inputs, const limits &l, error_info &error) const {
        return evaluate_many_(inputs, &l, &error);
    }

    // Check that every action the rule set names is registered.
    CLEANRULES_THROWING(void check_actions(const actions &registered) const {
        check_actions_(registered, nullptr);
    })
    bool check_actions(const actions &registered, error_info &error) const {
        return check_actions_(registered, &error);
    }

    // Evaluate, then call the actions of the selected rules, in rule order, on
    // this thread. The first that fails stops the run with an `action_error`,
    // which gives the decision; an exception the action threw is nested in it
    // (or, with an error_info, its `cause()`).
    CLEANRULES_THROWING(decision run(const actions &registered, value_ref input) const {
        return run_(registered, input, nullptr, nullptr);
    })
    CLEANRULES_THROWING(decision run(const actions &registered, value_ref input,
                                     const limits &l) const {
        return run_(registered, input, &l, nullptr);
    })
    decision run(const actions &registered, value_ref input, error_info &error) const {
        return run_(registered, input, nullptr, &error);
    }
    decision run(const actions &registered, value_ref input, const limits &l,
                 error_info &error) const {
        return run_(registered, input, &l, &error);
    }

    // Read a recorded decision, in JSON or YAML, made by this rule set.
    CLEANRULES_THROWING([[nodiscard]] decision read_decision(std::string_view text,
                                                             format f) const {
        return read_decision_(text, f, nullptr, nullptr);
    })
    CLEANRULES_THROWING([[nodiscard]] decision read_decision(std::string_view text, format f,
                                                             const limits &l) const {
        return read_decision_(text, f, &l, nullptr);
    })
    [[nodiscard]] decision read_decision(std::string_view text, format f,
                                         error_info &error) const {
        return read_decision_(text, f, nullptr, &error);
    }
    [[nodiscard]] decision read_decision(std::string_view text, format f, const limits &l,
                                         error_info &error) const {
        return read_decision_(text, f, &l, &error);
    }

    explicit operator bool() const noexcept { return handle_ != nullptr; }
    const cr_ruleset *c_ptr() const noexcept { return handle_.get(); }

private:
    const cr_ruleset *raw() const noexcept { return handle_.get(); }

    static ruleset adopt(cr_ruleset *raw) noexcept {
        ruleset r;
        r.handle_.reset(raw);
        return r;
    }
    static ruleset read_(std::string_view text, format f, const limits *l, error_info *sink) {
        cr_ruleset *out = nullptr;
        detail::check(sink, [&](cr_error **e) {
            return cr_ruleset_read(text.data(), text.size(), static_cast<cr_format>(f),
                                   detail::limits_of(l), &out, e);
        });
        return adopt(out);
    }
    static ruleset from_document_(const document &d, error_info *sink) {
        cr_ruleset *out = nullptr;
        detail::check(sink,
                      [&](cr_error **e) { return cr_ruleset_from_document(d.c_ptr(), &out, e); });
        return adopt(out);
    }
    std::string_view rule_name_(std::size_t rule, error_info *sink) const {
        const char *name = nullptr;
        std::size_t len = 0;
        detail::check(sink,
                      [&](cr_error **e) { return cr_ruleset_rule_name(raw(), rule, &name, &len, e); });
        return detail::view(name, len);
    }
    std::string write_(format f, error_info *sink) const {
        return detail::text(sink, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_ruleset_write(raw(), static_cast<cr_format>(f), o, n, e);
        });
    }
    std::string summary_(format f, error_info *sink) const {
        return detail::text(sink, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_ruleset_summary(raw(), static_cast<cr_format>(f), o, n, e);
        });
    }
    static ruleset make_(const rule_refs &rules, const table_settings &s, error_info *sink);
    std::size_t rule_position_(std::string_view name, error_info *sink) const {
        std::size_t position = 0;
        detail::check(sink, [&](cr_error **e) {
            return cr_ruleset_rule_position(raw(), name.data(), name.size(), &position, e);
        });
        return position;
    }
    std::string summary_actions_(const std::vector<bool> &actions, format f,
                                 error_info *sink) const {
        // `std::vector<bool>` packs its bits: the flags are copied to bools.
        std::unique_ptr<bool[]> flags(new bool[actions.size() + 1]);
        for (std::size_t i = 0; i < actions.size(); ++i) {
            flags[i] = actions[i];
        }
        return detail::text(sink, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_ruleset_summary_actions(raw(), flags.get(), actions.size(),
                                              static_cast<cr_format>(f), o, n, e);
        });
    }
    value field_paths_(error_info *sink) const {
        cr_value *out = nullptr;
        detail::check(sink, [&](cr_error **e) { return cr_ruleset_field_paths(raw(), &out, e); });
        return value::adopt(out);
    }
    decision evaluate_(value_ref input, const limits *l, error_info *sink) const {
        cr_decision *out = nullptr;
        detail::check(sink, [&](cr_error **e) {
            return cr_ruleset_evaluate_value(raw(), input.c_ptr(), detail::limits_of(l), &out, e);
        });
        return decision::adopt(out);
    }
    decision evaluate_json_(std::string_view input, const limits *l, error_info *sink) const {
        cr_decision *out = nullptr;
        detail::check(sink, [&](cr_error **e) {
            return cr_ruleset_evaluate(raw(), input.data(), input.size(), detail::limits_of(l),
                                       &out, e);
        });
        return decision::adopt(out);
    }
    batch evaluate_many_(value_ref inputs, const limits *l, error_info *sink) const {
        batch b;
        cr_batch *out = nullptr;
        detail::check(sink, [&](cr_error **e) {
            return cr_ruleset_evaluate_many(raw(), inputs.c_ptr(), detail::limits_of(l), &out, e);
        });
        b.handle_.reset(out);
        return b;
    }
    bool check_actions_(const actions &registered, error_info *sink) const {
        return detail::check(sink, [&](cr_error **e) {
            return cr_ruleset_check_actions(raw(), registered.c_ptr(), e);
        });
    }
    decision run_(const actions &registered, value_ref input, const limits *l,
                  error_info *sink) const {
        detail::run_frame frame;
        cr_decision *out = nullptr;
        cr_error *raw_error = nullptr;
        cr_status status = cr_ruleset_run(raw(), registered.c_ptr(), input.c_ptr(),
                                          detail::limits_of(l), &out, &raw_error);
        detail::finish(status, raw_error, sink, frame.thrown());
        return decision::adopt(out);
    }
    decision read_decision_(std::string_view text, format f, const limits *l,
                            error_info *sink) const {
        cr_decision *out = nullptr;
        detail::check(sink, [&](cr_error **e) {
            return cr_ruleset_read_decision(raw(), text.data(), text.size(),
                                            static_cast<cr_format>(f), detail::limits_of(l), &out,
                                            e);
        });
        return decision::adopt(out);
    }

    detail::owned<cr_ruleset, cr_ruleset_free> handle_;
};

// One node: a condition or a value, read on its own and evaluated on one
// input with its result tree (spec/ast.md, *Evaluating one node*).
class node {
public:
    node() noexcept = default;

    // Read a node: JSON or YAML (the node itself), or the rule syntax, such as
    // `customer.years > 5` or `order.total * 0.9`.
    CLEANRULES_THROWING([[nodiscard]] static node read(std::string_view text, format f) {
        return read_(text, f, nullptr, nullptr);
    })
    CLEANRULES_THROWING([[nodiscard]] static node read(std::string_view text, format f,
                                                       const limits &l) {
        return read_(text, f, &l, nullptr);
    })
    [[nodiscard]] static node read(std::string_view text, format f, error_info &error) {
        return read_(text, f, nullptr, &error);
    }
    [[nodiscard]] static node read(std::string_view text, format f, const limits &l,
                                   error_info &error) {
        return read_(text, f, &l, &error);
    }

    // Write it: JSON by default (the node itself), YAML or the rule syntax.
    CLEANRULES_THROWING(std::string write(format f = format::json) const {
        return write_(f, nullptr);
    })
    std::string write(error_info &error) const { return write_(format::json, &error); }
    std::string write(format f, error_info &error) const { return write_(f, &error); }

    // Evaluate it as a condition, under a logic by name, strict or not: its
    // result tree as JSON, whose root holds the truth value.
    CLEANRULES_THROWING([[nodiscard]] std::string evaluate_tree(
        value_ref input, std::string_view logic = "boolean", bool strict = false) const {
        return tree_(input, &logic, strict, nullptr);
    })
    [[nodiscard]] std::string evaluate_tree(value_ref input, error_info &error) const {
        std::string_view logic = "boolean";
        return tree_(input, &logic, false, &error);
    }
    [[nodiscard]] std::string evaluate_tree(value_ref input, std::string_view logic,
                                            error_info &error) const {
        return tree_(input, &logic, false, &error);
    }
    [[nodiscard]] std::string evaluate_tree(value_ref input, std::string_view logic, bool strict,
                                            error_info &error) const {
        return tree_(input, &logic, strict, &error);
    }

    // Evaluate it as a value, strict or not: its result tree, whose root holds
    // the value.
    CLEANRULES_THROWING([[nodiscard]] std::string evaluate_value_tree(value_ref input,
                                                                      bool strict = false) const {
        return tree_(input, nullptr, strict, nullptr);
    })
    [[nodiscard]] std::string evaluate_value_tree(value_ref input, error_info &error) const {
        return tree_(input, nullptr, false, &error);
    }
    [[nodiscard]] std::string evaluate_value_tree(value_ref input, bool strict,
                                                  error_info &error) const {
        return tree_(input, nullptr, strict, &error);
    }

    // The paths of the input that the node reads, in preorder and each once: a
    // list of paths, each a list of strings (keys) and ints (indexes).
    CLEANRULES_THROWING([[nodiscard]] value field_paths() const { return field_paths_(nullptr); })
    [[nodiscard]] value field_paths(error_info &error) const { return field_paths_(&error); }

    // --- Building (spec/ast.md, *Construction rules*): the core checks each
    // node and throws an `expression_error` for one it rejects. Operands are
    // copied.

    // An operand of `and_` and `or_`: a named node or a temporary, which lives
    // until the call returns.
    class ref {
    public:
        ref(const node &n) noexcept : ptr_(&n) {}
        const node &get() const noexcept { return *ptr_; }

    private:
        const node *ptr_;
    };
    // Operands of `and_` and `or_`, in order: `{a, node::compare(...)}`.
    using refs = std::vector<ref>;

    // A field: a list of strings (keys, never split at dots) and ints.
    CLEANRULES_THROWING([[nodiscard]] static node field(value_ref path) {
        return make_(nullptr, [&](cr_node **o, cr_error **e) { return cr_node_new_field(path.c_ptr(), o, e); });
    })
    [[nodiscard]] static node field(value_ref path, error_info &error) {
        return make_(&error, [&](cr_node **o, cr_error **e) { return cr_node_new_field(path.c_ptr(), o, e); });
    }
    // A constant: null, a boolean, a number, a string, a date or a datetime.
    CLEANRULES_THROWING([[nodiscard]] static node constant(value_ref v) {
        return make_(nullptr, [&](cr_node **o, cr_error **e) { return cr_node_new_constant(v.c_ptr(), o, e); });
    })
    [[nodiscard]] static node constant(value_ref v, error_info &error) {
        return make_(&error, [&](cr_node **o, cr_error **e) { return cr_node_new_constant(v.c_ptr(), o, e); });
    }
    // From anything a `value` is made of: `node::constant(5)`.
    CLEANRULES_THROWING([[nodiscard]] static node constant(const value &v) {
        return constant(v.ref());
    })
    [[nodiscard]] static node constant(const value &v, error_info &error) {
        return constant(v.ref(), error);
    }
    // `left op right`: `op` is `eq`, `ne`, `lt`, `le`, `gt` or `ge`.
    CLEANRULES_THROWING([[nodiscard]] static node compare(std::string_view op, const node &left,
                                                          const node &right) {
        return compare_(op, left, right, nullptr);
    })
    [[nodiscard]] static node compare(std::string_view op, const node &left, const node &right,
                                      error_info &error) {
        return compare_(op, left, right, &error);
    }
    // `left op right`: `op` is `add`, `sub`, `mul` or `div`.
    CLEANRULES_THROWING([[nodiscard]] static node arith(std::string_view op, const node &left,
                                                        const node &right) {
        return arith_(op, left, right, nullptr);
    })
    [[nodiscard]] static node arith(std::string_view op, const node &left, const node &right,
                                    error_info &error) {
        return arith_(op, left, right, &error);
    }
    // The conjunction and the disjunction of two or more conditions.
    CLEANRULES_THROWING([[nodiscard]] static node and_(const refs &operands) {
        return connective_(cr_node_new_and, operands, nullptr);
    })
    [[nodiscard]] static node and_(const refs &operands, error_info &error) {
        return connective_(cr_node_new_and, operands, &error);
    }
    CLEANRULES_THROWING([[nodiscard]] static node or_(const refs &operands) {
        return connective_(cr_node_new_or, operands, nullptr);
    })
    [[nodiscard]] static node or_(const refs &operands, error_info &error) {
        return connective_(cr_node_new_or, operands, &error);
    }
    // `not`, `is_null`, `abs` and `neg` of one operand.
    CLEANRULES_THROWING([[nodiscard]] static node not_(const node &n) { return unary_(cr_node_new_not, n, nullptr); })
    [[nodiscard]] static node not_(const node &n, error_info &error) { return unary_(cr_node_new_not, n, &error); }
    CLEANRULES_THROWING([[nodiscard]] static node is_null(const node &n) { return unary_(cr_node_new_is_null, n, nullptr); })
    [[nodiscard]] static node is_null(const node &n, error_info &error) { return unary_(cr_node_new_is_null, n, &error); }
    CLEANRULES_THROWING([[nodiscard]] static node abs(const node &n) { return unary_(cr_node_new_abs, n, nullptr); })
    [[nodiscard]] static node abs(const node &n, error_info &error) { return unary_(cr_node_new_abs, n, &error); }
    CLEANRULES_THROWING([[nodiscard]] static node neg(const node &n) { return unary_(cr_node_new_neg, n, nullptr); })
    [[nodiscard]] static node neg(const node &n, error_info &error) { return unary_(cr_node_new_neg, n, &error); }
    // Membership in a list of constants, kept sorted and without repeats.
    CLEANRULES_THROWING([[nodiscard]] static node is_in(const node &n, value_ref values) {
        return is_in_(n, values, nullptr);
    })
    [[nodiscard]] static node is_in(const node &n, value_ref values, error_info &error) {
        return is_in_(n, values, &error);
    }
    // `low <= n <= high`; `closed` is `both` (also when empty), `left`,
    // `right` or `none`.
    CLEANRULES_THROWING([[nodiscard]] static node between(const node &n, const node &low,
                                                          const node &high,
                                                          std::string_view closed = {}) {
        return between_(n, low, high, closed, nullptr);
    })
    [[nodiscard]] static node between(const node &n, const node &low, const node &high,
                                      std::string_view closed, error_info &error) {
        return between_(n, low, high, closed, &error);
    }
    // A regular expression search (Rust's `regex` syntax).
    CLEANRULES_THROWING([[nodiscard]] static node matches(const node &n, std::string_view pattern) {
        return matches_(n, pattern, nullptr);
    })
    [[nodiscard]] static node matches(const node &n, std::string_view pattern, error_info &error) {
        return matches_(n, pattern, &error);
    }
    // A literal string test: `op` is `starts_with`, `ends_with` or `contains`.
    CLEANRULES_THROWING([[nodiscard]] static node string_test(std::string_view op, const node &n,
                                                              std::string_view text) {
        return string_test_(op, n, text, nullptr);
    })
    [[nodiscard]] static node string_test(std::string_view op, const node &n,
                                          std::string_view text, error_info &error) {
        return string_test_(op, n, text, &error);
    }

    // --- What the node says about itself.

    // Its tag (`gt`, `and`...) and kind (`condition`, `value` or `any`).
    std::string_view tag() const noexcept { return name_(cr_node_tag(handle_.get())); }
    std::string_view kind() const noexcept { return name_(cr_node_kind(handle_.get())); }
    // Its direct children, copied, in order.
    CLEANRULES_THROWING([[nodiscard]] std::vector<node> children() const { return children_(nullptr); })
    [[nodiscard]] std::vector<node> children(error_info &error) const { return children_(&error); }
    // Its own data: a field's path, a constant's value, an `in`'s values, a
    // `between`'s `closed`, a pattern or a text; null otherwise.
    CLEANRULES_THROWING([[nodiscard]] value detail() const { return detail_(nullptr); })
    [[nodiscard]] value detail(error_info &error) const { return detail_(&error); }
    // Structural identity (spec/ast.md), and a hash consistent with it.
    bool same_as(const node &other) const noexcept {
        return cr_node_same(handle_.get(), other.handle_.get());
    }
    std::uint64_t hash() const noexcept { return cr_node_hash(handle_.get()); }

    explicit operator bool() const noexcept { return handle_ != nullptr; }
    const cr_node *c_ptr() const noexcept { return handle_.get(); }

private:
    template <class Call>
    static node make_(error_info *sink, Call &&call) {
        node made;
        cr_node *out = nullptr;
        detail::check(sink, [&](cr_error **e) { return std::forward<Call>(call)(&out, e); });
        made.handle_.reset(out);
        return made;
    }
    static node compare_(std::string_view op, const node &l, const node &r, error_info *sink) {
        return make_(sink, [&](cr_node **o, cr_error **e) {
            return cr_node_new_compare(op.data(), op.size(), l.c_ptr(), r.c_ptr(), o, e);
        });
    }
    static node arith_(std::string_view op, const node &l, const node &r, error_info *sink) {
        return make_(sink, [&](cr_node **o, cr_error **e) {
            return cr_node_new_arith(op.data(), op.size(), l.c_ptr(), r.c_ptr(), o, e);
        });
    }
    static node connective_(cr_status (*join)(const cr_node *const *, std::size_t, cr_node **,
                                              cr_error **),
                            const refs &operands, error_info *sink) {
        std::vector<const cr_node *> pointers;
        pointers.reserve(operands.size());
        for (const ref &operand : operands) {
            pointers.push_back(operand.get().c_ptr());
        }
        return make_(sink, [&](cr_node **o, cr_error **e) {
            return join(pointers.data(), pointers.size(), o, e);
        });
    }
    static node unary_(cr_status (*make)(const cr_node *, cr_node **, cr_error **), const node &n,
                       error_info *sink) {
        return make_(sink, [&](cr_node **o, cr_error **e) { return make(n.c_ptr(), o, e); });
    }
    static node is_in_(const node &n, value_ref values, error_info *sink) {
        return make_(sink, [&](cr_node **o, cr_error **e) {
            return cr_node_new_is_in(n.c_ptr(), values.c_ptr(), o, e);
        });
    }
    static node between_(const node &n, const node &low, const node &high, std::string_view closed,
                         error_info *sink) {
        return make_(sink, [&](cr_node **o, cr_error **e) {
            return cr_node_new_between(n.c_ptr(), low.c_ptr(), high.c_ptr(),
                                   closed.empty() ? nullptr : closed.data(), closed.size(), o, e);
        });
    }
    static node matches_(const node &n, std::string_view pattern, error_info *sink) {
        return make_(sink, [&](cr_node **o, cr_error **e) {
            return cr_node_new_matches(n.c_ptr(), pattern.data(), pattern.size(), o, e);
        });
    }
    static node string_test_(std::string_view op, const node &n, std::string_view text,
                             error_info *sink) {
        return make_(sink, [&](cr_node **o, cr_error **e) {
            return cr_node_new_string_test(op.data(), op.size(), n.c_ptr(), text.data(), text.size(),
                                       o, e);
        });
    }
    static std::string_view name_(const char *name) noexcept {
        return name != nullptr ? std::string_view(name) : std::string_view();
    }
    std::vector<node> children_(error_info *sink) const {
        std::vector<node> children;
        std::size_t count = cr_node_children_len(handle_.get());
        for (std::size_t i = 0; i < count; ++i) {
            node child = make_(sink, [&](cr_node **o, cr_error **e) {
                return cr_node_child(handle_.get(), i, o, e);
            });
            if (!child) {
                return {};
            }
            children.push_back(std::move(child));
        }
        if (handle_ == nullptr) {
            // The error of a NULL node.
            detail::check(sink, [&](cr_error **e) { return cr_node_child(nullptr, 0, nullptr, e); });
        }
        return children;
    }
    value detail_(error_info *sink) const {
        cr_value *out = nullptr;
        detail::check(sink, [&](cr_error **e) { return cr_node_detail(handle_.get(), &out, e); });
        return value::adopt(out);
    }
    value field_paths_(error_info *sink) const {
        cr_value *out = nullptr;
        detail::check(sink,
                      [&](cr_error **e) { return cr_node_field_paths(handle_.get(), &out, e); });
        return value::adopt(out);
    }
    static node read_(std::string_view text, format f, const limits *l, error_info *sink) {
        node n;
        cr_node *out = nullptr;
        detail::check(sink, [&](cr_error **e) {
            return cr_node_read(text.data(), text.size(), static_cast<cr_format>(f),
                                detail::limits_of(l), &out, e);
        });
        n.handle_.reset(out);
        return n;
    }
    std::string write_(format f, error_info *sink) const {
        return detail::text(sink, [&](char **o, std::size_t *n, cr_error **e) {
            return cr_node_write(handle_.get(), static_cast<cr_format>(f), o, n, e);
        });
    }
    std::string tree_(value_ref input, const std::string_view *logic, bool strict,
                      error_info *sink) const {
        return detail::text(sink, [&](char **o, std::size_t *n, cr_error **e) {
            if (logic == nullptr) {
                return cr_node_evaluate_value_tree(handle_.get(), input.c_ptr(), strict, o, n, e);
            }
            return cr_node_evaluate_tree(handle_.get(), input.c_ptr(), logic->data(),
                                         logic->size(), strict, o, n, e);
        });
    }

    detail::owned<cr_node, cr_node_free> handle_;
};

// The settings of a decision table, by name, as in JSON; empty means the
// default: `first`, no aggregate, `boolean`.
struct table_settings {
    std::string_view hit_policy;
    std::string_view aggregate;
    std::string_view logic;
    bool strict = false;
};

// The settings of a rule set built in code (`ruleset::make`): the same.
using ruleset_settings = table_settings;

// A rule built in code (spec/rules.md, *Rules*): a name and a condition,
// then each `with_*` gives a new rule, checked by the core.
class rule {
public:
    rule() noexcept = default;

    // A rule with a name and a condition (copied).
    CLEANRULES_THROWING([[nodiscard]] static rule make(std::string_view name, const node &when) {
        return make_(nullptr, [&](cr_rule **o, cr_error **e) {
            return cr_rule_new(name.data(), name.size(), when.c_ptr(), o, e);
        });
    })
    [[nodiscard]] static rule make(std::string_view name, const node &when, error_info &error) {
        return make_(&error, [&](cr_rule **o, cr_error **e) {
            return cr_rule_new(name.data(), name.size(), when.c_ptr(), o, e);
        });
    }
    // With `key` in its payload, given a value node (a repeated key keeps its
    // place and takes the new value).
    CLEANRULES_THROWING([[nodiscard]] rule with_payload(std::string_view key, const node &v) const {
        return payload_(key, v, nullptr);
    })
    [[nodiscard]] rule with_payload(std::string_view key, const node &v, error_info &error) const {
        return payload_(key, v, &error);
    }
    CLEANRULES_THROWING([[nodiscard]] rule with_priority(std::int64_t priority) const {
        return priority_(priority, nullptr);
    })
    [[nodiscard]] rule with_priority(std::int64_t priority, error_info &error) const {
        return priority_(priority, &error);
    }
    // With these tags (a list of strings), kept sorted and without repeats.
    CLEANRULES_THROWING([[nodiscard]] rule with_tags(value_ref tags) const {
        return value_step_(cr_rule_with_tags, tags, nullptr);
    })
    [[nodiscard]] rule with_tags(value_ref tags, error_info &error) const {
        return value_step_(cr_rule_with_tags, tags, &error);
    }
    // With this metadata: a map of data values.
    CLEANRULES_THROWING([[nodiscard]] rule with_metadata(value_ref metadata) const {
        return value_step_(cr_rule_with_metadata, metadata, nullptr);
    })
    [[nodiscard]] rule with_metadata(value_ref metadata, error_info &error) const {
        return value_step_(cr_rule_with_metadata, metadata, &error);
    }
    CLEANRULES_THROWING([[nodiscard]] rule with_description(std::string_view text) const {
        return text_step_(cr_rule_with_description, text, nullptr);
    })
    [[nodiscard]] rule with_description(std::string_view text, error_info &error) const {
        return text_step_(cr_rule_with_description, text, &error);
    }
    // Naming its action, which a registry resolves (`actions`).
    CLEANRULES_THROWING([[nodiscard]] rule with_action(std::string_view name) const {
        return text_step_(cr_rule_with_action, name, nullptr);
    })
    [[nodiscard]] rule with_action(std::string_view name, error_info &error) const {
        return text_step_(cr_rule_with_action, name, &error);
    }

    // The tags as the core keeps them, and the metadata.
    CLEANRULES_THROWING([[nodiscard]] value tags() const { return read_(cr_rule_tags, nullptr); })
    [[nodiscard]] value tags(error_info &error) const { return read_(cr_rule_tags, &error); }
    CLEANRULES_THROWING([[nodiscard]] value metadata() const { return read_(cr_rule_metadata, nullptr); })
    [[nodiscard]] value metadata(error_info &error) const { return read_(cr_rule_metadata, &error); }

    explicit operator bool() const noexcept { return handle_ != nullptr; }
    const cr_rule *c_ptr() const noexcept { return handle_.get(); }

private:
    template <class Call>
    static rule make_(error_info *sink, Call &&call) {
        rule made;
        cr_rule *out = nullptr;
        detail::check(sink, [&](cr_error **e) { return std::forward<Call>(call)(&out, e); });
        made.handle_.reset(out);
        return made;
    }
    rule payload_(std::string_view key, const node &v, error_info *sink) const {
        return make_(sink, [&](cr_rule **o, cr_error **e) {
            return cr_rule_with_payload(c_ptr(), key.data(), key.size(), v.c_ptr(), o, e);
        });
    }
    rule priority_(std::int64_t priority, error_info *sink) const {
        return make_(sink, [&](cr_rule **o, cr_error **e) {
            return cr_rule_with_priority(c_ptr(), priority, o, e);
        });
    }
    rule value_step_(cr_status (*step)(const cr_rule *, const cr_value *, cr_rule **, cr_error **),
                     value_ref v, error_info *sink) const {
        return make_(sink, [&](cr_rule **o, cr_error **e) { return step(c_ptr(), v.c_ptr(), o, e); });
    }
    rule text_step_(cr_status (*step)(const cr_rule *, const char *, std::size_t, cr_rule **,
                                      cr_error **),
                    std::string_view text, error_info *sink) const {
        return make_(sink, [&](cr_rule **o, cr_error **e) {
            return step(c_ptr(), text.data(), text.size(), o, e);
        });
    }
    value read_(cr_status (*read)(const cr_rule *, cr_value **, cr_error **),
                error_info *sink) const {
        cr_value *out = nullptr;
        detail::check(sink, [&](cr_error **e) { return read(c_ptr(), &out, e); });
        return value::adopt(out);
    }

    detail::owned<cr_rule, cr_rule_free> handle_;
};

inline ruleset ruleset::make_(const rule_refs &rules, const table_settings &s, error_info *sink) {
    std::vector<const cr_rule *> pointers;
    pointers.reserve(rules.size());
    for (const rule_ref &r : rules) {
        pointers.push_back(r.get().c_ptr());
    }
    auto name = [](std::string_view n) { return n.empty() ? nullptr : n.data(); };
    cr_ruleset *out = nullptr;
    detail::check(sink, [&](cr_error **e) {
        return cr_ruleset_new(pointers.data(), pointers.size(), name(s.hit_policy),
                              s.hit_policy.size(), name(s.aggregate), s.aggregate.size(),
                              name(s.logic), s.logic.size(), s.strict, &out, e);
    });
    return adopt(out);
}
#if CLEANRULES_EXCEPTIONS
inline ruleset ruleset::make(const rule_refs &rules) { return make_(rules, table_settings(), nullptr); }
inline ruleset ruleset::make(const rule_refs &rules, const table_settings &settings) {
    return make_(rules, settings, nullptr);
}
#endif
inline ruleset ruleset::make(const rule_refs &rules, error_info &error) {
    return make_(rules, table_settings(), &error);
}
inline ruleset ruleset::make(const rule_refs &rules, const table_settings &settings,
                             error_info &error) {
    return make_(rules, settings, &error);
}

// A decision table built from a grid (spec/tables.md, *Grids*): a header and
// rows of cells, pushed one at a time. After a row fails, the grid can only
// be dropped.
class grid {
public:
    grid() noexcept = default;

    // Start a table. The header is a list of strings, each read as a CSV
    // header field: `RULE`, `PRIORITY`, `DESCRIPTION`, `THEN key [IN (...)]`
    // or an input expression.
    CLEANRULES_THROWING([[nodiscard]] static grid make(value_ref header,
                                                       const table_settings &settings = {}) {
        return make_(header, settings, nullptr, nullptr);
    })
    CLEANRULES_THROWING([[nodiscard]] static grid make(value_ref header,
                                                       const table_settings &settings,
                                                       const limits &l) {
        return make_(header, settings, &l, nullptr);
    })
    [[nodiscard]] static grid make(value_ref header, error_info &error) {
        return make_(header, table_settings(), nullptr, &error);
    }
    [[nodiscard]] static grid make(value_ref header, const table_settings &settings,
                                   error_info &error) {
        return make_(header, settings, nullptr, &error);
    }
    [[nodiscard]] static grid make(value_ref header, const table_settings &settings,
                                   const limits &l, error_info &error) {
        return make_(header, settings, &l, &error);
    }

    // Add a row: a list of cells in column order. A string is text, read as a
    // CSV field (`'VIP'` is the string, `VIP` the field); null is an empty
    // cell; any other value is a constant.
    CLEANRULES_THROWING(grid &push_row(value_ref row) {
        push_row_(row, nullptr);
        return *this;
    })
    bool push_row(value_ref row, error_info &error) { return push_row_(row, &error); }

    // Check and compile the table into a document, which
    // `ruleset::from_document` turns into a rule set. The grid is used up.
    CLEANRULES_THROWING([[nodiscard]] document finish() { return finish_(nullptr); })
    [[nodiscard]] document finish(error_info &error) { return finish_(&error); }

    explicit operator bool() const noexcept { return handle_ != nullptr; }

private:
    static const char *name_or_null(std::string_view name) noexcept {
        return name.empty() ? nullptr : name.data();
    }
    static grid make_(value_ref header, const table_settings &s, const limits *l,
                      error_info *sink) {
        grid g;
        cr_grid *out = nullptr;
        detail::check(sink, [&](cr_error **e) {
            return cr_grid_new(name_or_null(s.hit_policy), s.hit_policy.size(),
                               name_or_null(s.aggregate), s.aggregate.size(),
                               name_or_null(s.logic), s.logic.size(), s.strict, header.c_ptr(),
                               detail::limits_of(l), &out, e);
        });
        g.handle_.reset(out);
        return g;
    }
    bool push_row_(value_ref row, error_info *sink) {
        return detail::check(
            sink, [&](cr_error **e) { return cr_grid_push_row(handle_.get(), row.c_ptr(), e); });
    }
    document finish_(error_info *sink) {
        document d;
        cr_document *out = nullptr;
        // The grid is consumed, even when finishing fails.
        detail::check(sink,
                      [&](cr_error **e) { return cr_grid_finish(handle_.release(), &out, e); });
        d.handle_.reset(out);
        return d;
    }

    detail::owned<cr_grid, cr_grid_free> handle_;
};

}  // inline namespace eh / noeh
}  // namespace cleanrules

// Nodes as keys of unordered containers, by structural identity
// (spec/ast.md): `std::unordered_set<cleanrules::node>`.
namespace std {
template <>
struct hash<cleanrules::node> {
    std::size_t operator()(const cleanrules::node &n) const noexcept {
        return static_cast<std::size_t>(n.hash());
    }
};
template <>
struct equal_to<cleanrules::node> {
    bool operator()(const cleanrules::node &a, const cleanrules::node &b) const noexcept {
        return a.same_as(b);
    }
};
}  // namespace std

#undef CLEANRULES_THROWING

#endif  // CLEANRULES_HPP
