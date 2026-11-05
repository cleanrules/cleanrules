package io.github.cleanrules.internal;

import static java.lang.foreign.ValueLayout.ADDRESS;
import static java.lang.foreign.ValueLayout.JAVA_BOOLEAN;
import static java.lang.foreign.ValueLayout.JAVA_BYTE;
import static java.lang.foreign.ValueLayout.JAVA_DOUBLE;
import static java.lang.foreign.ValueLayout.JAVA_INT;
import static java.lang.foreign.ValueLayout.JAVA_LONG;

import io.github.cleanrules.CleanRules;
import io.github.cleanrules.DocumentKind;
import io.github.cleanrules.Format;
import io.github.cleanrules.Truth;
import java.lang.foreign.Arena;
import java.lang.foreign.FunctionDescriptor;
import java.lang.foreign.Linker;
import java.lang.foreign.MemoryLayout;
import java.lang.foreign.MemorySegment;
import java.lang.foreign.SymbolLookup;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.util.ArrayList;
import java.util.List;

/**
 * The C ABI ({@code crates/ffi/include/cleanrules.h}), bound by hand: one
 * downcall handle per function, and one static method per function that calls
 * it with {@code invokeExact}.
 *
 * <p>The library is found, in order, at the path of the system property
 * {@code cleanrules.library.path} or the environment variable
 * {@code CLEANRULES_LIBRARY_PATH} (a file, or a directory holding
 * {@code System.mapLibraryName("cleanrules_ffi")}), then with
 * {@code System.loadLibrary("cleanrules_ffi")} ({@code java.library.path}),
 * then by the system's dynamic loader. Loading never throws while this class
 * initialises: a failure is kept and {@link #ensureLoaded()} throws it as an
 * {@link UnsatisfiedLinkError} each time, so the message is never lost to a
 * {@code NoClassDefFoundError}. Every restricted call (loading, linking, upcall
 * stubs) happens in that guard or after it succeeded, so a program without
 * native access for the binding gets that error too, saying which flag
 * enables it.
 *
 * <p>{@code size_t} is bound as {@code long}: the binding needs a 64-bit
 * platform, which loading checks.
 *
 * <p>This is the only class that calls restricted methods of the FFM API
 * (loading, linking, and {@code reinterpret} to the exact length the library
 * gives for a text), hence the one {@code @SuppressWarnings("restricted")}.
 */
@SuppressWarnings("restricted")
public final class Native {
    private Native() {}

    /** The ABI version this binding is written for: {@code CR_ABI_VERSION}. */
    public static final int ABI_VERSION = 1;

    public static final String LIBRARY = "cleanrules_ffi";
    public static final String PROPERTY = "cleanrules.library.path";
    public static final String VARIABLE = "CLEANRULES_LIBRARY_PATH";

    // Statuses, kinds and reasons of the header.
    public static final int CR_OK = 0;
    public static final int CR_INVALID_ARGUMENT = -1;
    public static final int CR_INTERNAL_ERROR = -2;
    public static final int CR_ACTION_ERROR = 14;

    public static final int CR_VALUE_NULL = 1;
    public static final int CR_VALUE_BOOL = 2;
    public static final int CR_VALUE_INT = 3;
    public static final int CR_VALUE_FLOAT = 4;
    public static final int CR_VALUE_DECIMAL = 5;
    public static final int CR_VALUE_STRING = 6;
    public static final int CR_VALUE_DATE = 7;
    public static final int CR_VALUE_NAIVE_DATETIME = 8;
    public static final int CR_VALUE_AWARE_DATETIME = 9;
    public static final int CR_VALUE_LIST = 10;
    public static final int CR_VALUE_MAP = 11;
    public static final int CR_VALUE_OPAQUE = 12;

    public static final int CR_REASON_INT_OVERFLOW = 2;
    public static final int CR_REASON_DECIMAL_OVERFLOW = 3;
    public static final int CR_REASON_READ_ERROR = 4;

    public static final int CR_LIMIT_MAX_SIZE = 1;
    public static final int CR_LIMIT_MAX_NESTING = 2;
    public static final int CR_LIMIT_MAX_DEPTH = 3;
    public static final int CR_LIMIT_MAX_TABLE_SIZE = 4;
    public static final int CR_LIMIT_MAX_YAML_SIZE = 5;
    public static final int CR_LIMIT_MAX_TEXT_SIZE = 6;

    private static final Linker LINKER = Linker.nativeLinker();
    private static final MemoryLayout SIZE_T = JAVA_LONG;

    private static final SymbolLookup LOOKUP;
    private static final Path LOADED_FROM;
    private static final List<String> MISSING = new ArrayList<>();
    private static final Throwable LOAD_FAILURE;

    static {
        SymbolLookup lookup = null;
        Path from = null;
        Throwable failure = null;
        try {
            long sizeT = LINKER.canonicalLayouts().get("size_t").byteSize();
            if (sizeT != 8) {
                throw new UnsatisfiedLinkError(
                        "cleanrules needs a 64-bit platform; size_t has " + sizeT + " bytes here");
            }
            Path given = givenPath();
            if (given != null) {
                from = given;
                try {
                    lookup = SymbolLookup.libraryLookup(given, Arena.global());
                } catch (IllegalArgumentException cannotOpen) {
                    UnsatisfiedLinkError error = new UnsatisfiedLinkError(
                            "cannot load the cleanrules library at " + given + ": " + cannotOpen.getMessage());
                    error.initCause(cannotOpen);
                    throw error;
                }
            } else {
                lookup = searched();
            }
        } catch (IllegalCallerException denied) {
            failure = nativeAccessDenied(denied);
        } catch (Throwable thrown) {
            failure = thrown;
        }
        LOOKUP = lookup;
        LOADED_FROM = from;
        LOAD_FAILURE = failure;
    }

    /** The path the property or the variable gives, or null when neither is set. */
    private static Path givenPath() {
        String origin = "the system property " + PROPERTY;
        String text = System.getProperty(PROPERTY);
        if (text == null || text.isEmpty()) {
            origin = "the environment variable " + VARIABLE;
            text = System.getenv(VARIABLE);
        }
        if (text == null || text.isEmpty()) {
            return null;
        }
        Path path = Path.of(text);
        if (Files.isDirectory(path)) {
            path = path.resolve(System.mapLibraryName(LIBRARY));
        }
        if (!Files.isRegularFile(path)) {
            throw new UnsatisfiedLinkError(
                    "no cleanrules library at " + path + " (from " + origin + "); build it with"
                            + " `cargo build -p cleanrules-ffi` or fix the path");
        }
        return path.toAbsolutePath();
    }

    /** Why loading failed when native access is not enabled for the binding. */
    private static UnsatisfiedLinkError nativeAccessDenied(IllegalCallerException denied) {
        UnsatisfiedLinkError error = new UnsatisfiedLinkError(
                "native access is not enabled for io.github.cleanrules: run with"
                        + " --enable-native-access=io.github.cleanrules (module path) or ALL-UNNAMED (class path)");
        error.initCause(denied);
        return error;
    }

    /** The library from {@code java.library.path}, or else from the system's loader. */
    private static SymbolLookup searched() {
        try {
            System.loadLibrary(LIBRARY);
            return SymbolLookup.loaderLookup();
        } catch (IllegalCallerException denied) {
            throw nativeAccessDenied(denied);
        } catch (UnsatisfiedLinkError notInJavaPath) {
            try {
                return SymbolLookup.libraryLookup(System.mapLibraryName(LIBRARY), Arena.global());
            } catch (IllegalArgumentException notFound) {
                UnsatisfiedLinkError error = new UnsatisfiedLinkError(
                        "cannot find the cleanrules library ("
                                + System.mapLibraryName(LIBRARY)
                                + "): set the system property "
                                + PROPERTY
                                + " or the environment variable "
                                + VARIABLE
                                + " to its path, or put it on java.library.path");
                error.initCause(notInJavaPath);
                throw error;
            }
        }
    }

    /** Why linking failed, when it did. */
    private static Throwable linkFailure;

    /** A downcall handle for {@code name}; null when the library did not load. */
    private static MethodHandle bind(String name, FunctionDescriptor descriptor) {
        return bind(LOOKUP, name, descriptor);
    }

    private static MethodHandle bind(SymbolLookup lookup, String name, FunctionDescriptor descriptor) {
        if (lookup == null || linkFailure != null) {
            return null;
        }
        try {
            var symbol = lookup.find(name);
            if (symbol.isEmpty()) {
                MISSING.add(name);
                return null;
            }
            return LINKER.downcallHandle(symbol.get(), descriptor);
        } catch (IllegalCallerException denied) {
            linkFailure = nativeAccessDenied(denied);
            return null;
        } catch (RuntimeException thrown) {
            linkFailure = thrown;
            return null;
        }
    }

    private static FunctionDescriptor fn(MemoryLayout result, MemoryLayout... arguments) {
        return FunctionDescriptor.of(result, arguments);
    }

    private static FunctionDescriptor proc(MemoryLayout... arguments) {
        return FunctionDescriptor.ofVoid(arguments);
    }

    private static final MemoryLayout P = ADDRESS;
    private static final MemoryLayout I = JAVA_INT;
    private static final MemoryLayout L = JAVA_LONG;
    private static final MemoryLayout B = JAVA_BOOLEAN;

    // Version and errors.
    private static final MethodHandle ABI = bind("cr_abi_version", fn(I));
    private static final MethodHandle VERSION = bind("cr_version", fn(P));
    private static final MethodHandle FORMAT_VERSION = bind("cr_format_version", fn(I));
    private static final MethodHandle ERROR_CODE = bind("cr_error_code", fn(I, P));
    private static final MethodHandle ERROR_MESSAGE = bind("cr_error_message", fn(P, P));
    private static final MethodHandle ERROR_HINT = bind("cr_error_hint", fn(P, P));
    private static final MethodHandle ERROR_JSON = bind("cr_error_json", fn(P, P));
    private static final MethodHandle ERROR_DECISION = bind("cr_error_decision", fn(P, P));
    private static final MethodHandle ERROR_FREE = bind("cr_error_free", proc(P));
    private static final MethodHandle STRING_FREE = bind("cr_string_free", proc(P));

    // Limits.
    private static final MethodHandle LIMITS_NEW = bind("cr_limits_new", fn(P));
    private static final MethodHandle LIMITS_SET = bind("cr_limits_set", fn(I, P, I, SIZE_T, P));
    private static final MethodHandle LIMITS_GET = bind("cr_limits_get", fn(I, P, I, P, P));
    private static final MethodHandle LIMITS_FREE = bind("cr_limits_free", proc(P));

    // Documents.
    private static final MethodHandle DOCUMENT_READ = bind("cr_document_read", fn(I, P, SIZE_T, I, P, P, P));
    private static final MethodHandle DOCUMENT_KIND = bind("cr_document_kind", fn(I, P));
    private static final MethodHandle DOCUMENT_WRITE = bind("cr_document_write", fn(I, P, I, P, P, P));
    private static final MethodHandle DOCUMENT_FREE = bind("cr_document_free", proc(P));

    // Rule sets.
    private static final MethodHandle RULESET_READ = bind("cr_ruleset_read", fn(I, P, SIZE_T, I, P, P, P));
    private static final MethodHandle RULESET_FROM_DOCUMENT = bind("cr_ruleset_from_document", fn(I, P, P, P));
    private static final MethodHandle RULESET_WRITE = bind("cr_ruleset_write", fn(I, P, I, P, P, P));
    private static final MethodHandle RULESET_SUMMARY = bind("cr_ruleset_summary", fn(I, P, I, P, P, P));
    private static final MethodHandle RULESET_LEN = bind("cr_ruleset_len", fn(SIZE_T, P));
    private static final MethodHandle RULESET_RULE_NAME = bind("cr_ruleset_rule_name", fn(I, P, SIZE_T, P, P, P));
    private static final MethodHandle RULESET_HIT_POLICY = bind("cr_ruleset_hit_policy", fn(P, P));
    private static final MethodHandle RULESET_AGGREGATE = bind("cr_ruleset_aggregate", fn(P, P));
    private static final MethodHandle RULESET_LOGIC = bind("cr_ruleset_logic", fn(P, P));
    private static final MethodHandle RULESET_STRICT = bind("cr_ruleset_strict", fn(B, P));
    private static final MethodHandle RULESET_FIELD_PATHS = bind("cr_ruleset_field_paths", fn(I, P, P, P));
    private static final MethodHandle RULESET_EVALUATE = bind("cr_ruleset_evaluate", fn(I, P, P, SIZE_T, P, P, P));
    private static final MethodHandle RULESET_EVALUATE_VALUE = bind("cr_ruleset_evaluate_value", fn(I, P, P, P, P, P));
    private static final MethodHandle RULESET_EVALUATE_MANY = bind("cr_ruleset_evaluate_many", fn(I, P, P, P, P, P));
    private static final MethodHandle RULESET_READ_DECISION =
            bind("cr_ruleset_read_decision", fn(I, P, P, SIZE_T, I, P, P, P));
    private static final MethodHandle RULESET_CHECK_ACTIONS = bind("cr_ruleset_check_actions", fn(I, P, P, P));
    private static final MethodHandle RULESET_RUN = bind("cr_ruleset_run", fn(I, P, P, P, P, P, P));
    private static final MethodHandle RULESET_FREE = bind("cr_ruleset_free", proc(P));

    // Decisions and batches.
    private static final MethodHandle DECISION_LEN = bind("cr_decision_len", fn(SIZE_T, P));
    private static final MethodHandle DECISION_RULE_NAME = bind("cr_decision_rule_name", fn(I, P, SIZE_T, P, P, P));
    private static final MethodHandle DECISION_FIRED = bind("cr_decision_fired", fn(I, P, SIZE_T, P, P));
    private static final MethodHandle DECISION_TRUTH = bind("cr_decision_truth", fn(I, P, SIZE_T, P, P));
    private static final MethodHandle DECISION_SELECTED_LEN = bind("cr_decision_selected_len", fn(SIZE_T, P));
    private static final MethodHandle DECISION_SELECTED = bind("cr_decision_selected", fn(I, P, SIZE_T, P, P));
    private static final MethodHandle DECISION_OUTPUT = bind("cr_decision_output", fn(I, P, P, P, P));
    private static final MethodHandle DECISION_OUTPUTS = bind("cr_decision_outputs", fn(I, P, P, P, P));
    private static final MethodHandle DECISION_OUTPUT_VALUE = bind("cr_decision_output_value", fn(I, P, P, P));
    private static final MethodHandle DECISION_OUTPUTS_VALUE = bind("cr_decision_outputs_value", fn(I, P, P, P));
    private static final MethodHandle DECISION_RULE_OUTPUT = bind("cr_decision_rule_output", fn(I, P, SIZE_T, P, P));
    private static final MethodHandle DECISION_WRITE = bind("cr_decision_write", fn(I, P, I, P, P, P));
    private static final MethodHandle DECISION_EXPLAIN = bind("cr_decision_explain", fn(I, P, I, P, P, P));
    private static final MethodHandle DECISION_CLONE = bind("cr_decision_clone", fn(I, P, P, P));
    private static final MethodHandle DECISION_FREE = bind("cr_decision_free", proc(P));
    private static final MethodHandle BATCH_LEN = bind("cr_batch_len", fn(SIZE_T, P));
    private static final MethodHandle BATCH_GET = bind("cr_batch_get", fn(P, P, SIZE_T));
    private static final MethodHandle BATCH_FREE = bind("cr_batch_free", proc(P));

    // Nodes.
    private static final MethodHandle NODE_READ = bind("cr_node_read", fn(I, P, SIZE_T, I, P, P, P));
    private static final MethodHandle NODE_WRITE = bind("cr_node_write", fn(I, P, I, P, P, P));
    private static final MethodHandle NODE_FIELD_PATHS = bind("cr_node_field_paths", fn(I, P, P, P));
    private static final MethodHandle NODE_EVALUATE_TREE =
            bind("cr_node_evaluate_tree", fn(I, P, P, P, SIZE_T, B, P, P, P));
    private static final MethodHandle NODE_EVALUATE_VALUE_TREE =
            bind("cr_node_evaluate_value_tree", fn(I, P, P, B, P, P, P));
    private static final MethodHandle NODE_FREE = bind("cr_node_free", proc(P));

    // Actions.
    private static final MethodHandle ACTIONS_NEW = bind("cr_actions_new", fn(P));
    private static final MethodHandle ACTIONS_ADD = bind("cr_actions_add", fn(I, P, P, SIZE_T, P, P, P));
    private static final MethodHandle ACTIONS_FREE = bind("cr_actions_free", proc(P));
    private static final MethodHandle ACTION_CALL_DECISION = bind("cr_action_call_decision", fn(P, P));
    private static final MethodHandle ACTION_CALL_RULE = bind("cr_action_call_rule", fn(SIZE_T, P));
    private static final MethodHandle ACTION_CALL_FAIL = bind("cr_action_call_fail", proc(P, P, SIZE_T));

    // Values.
    private static final MethodHandle VALUE_NEW_NULL = bind("cr_value_new_null", fn(P));
    private static final MethodHandle VALUE_NEW_NULL_REASON = bind("cr_value_new_null_reason", fn(I, I, P, SIZE_T, P, P));
    private static final MethodHandle VALUE_NEW_BOOL = bind("cr_value_new_bool", fn(P, B));
    private static final MethodHandle VALUE_NEW_INT = bind("cr_value_new_int", fn(P, L));
    private static final MethodHandle VALUE_NEW_FLOAT = bind("cr_value_new_float", fn(P, JAVA_DOUBLE));
    private static final MethodHandle VALUE_NEW_DECIMAL = bind("cr_value_new_decimal", fn(I, P, SIZE_T, P, P));
    private static final MethodHandle VALUE_NEW_STRING = bind("cr_value_new_string", fn(I, P, SIZE_T, P, P));
    private static final MethodHandle VALUE_NEW_DATE = bind("cr_value_new_date", fn(I, I, I, I, P, P));
    private static final MethodHandle VALUE_NEW_NAIVE_DATETIME =
            bind("cr_value_new_naive_datetime", fn(I, I, I, I, I, I, I, I, P, P));
    private static final MethodHandle VALUE_NEW_AWARE_DATETIME =
            bind("cr_value_new_aware_datetime", fn(I, L, I, I, P, SIZE_T, P, P));
    private static final MethodHandle VALUE_NEW_LIST = bind("cr_value_new_list", fn(P, SIZE_T));
    private static final MethodHandle VALUE_NEW_MAP = bind("cr_value_new_map", fn(P));
    private static final MethodHandle VALUE_NEW_OPAQUE = bind("cr_value_new_opaque", fn(I, P, SIZE_T, L, P, P));
    private static final MethodHandle VALUE_LIST_SET = bind("cr_value_list_set", fn(I, P, SIZE_T, P, P));
    private static final MethodHandle VALUE_MAP_INSERT = bind("cr_value_map_insert", fn(I, P, P, SIZE_T, P, P));
    private static final MethodHandle VALUE_FREE = bind("cr_value_free", proc(P));
    private static final MethodHandle VALUE_FROM_JSON = bind("cr_value_from_json", fn(I, P, SIZE_T, P, P, P));
    private static final MethodHandle VALUE_KIND = bind("cr_value_kind", fn(I, P));
    private static final MethodHandle VALUE_LEN = bind("cr_value_len", fn(SIZE_T, P));
    private static final MethodHandle VALUE_GET_BOOL = bind("cr_value_get_bool", fn(I, P, P, P));
    private static final MethodHandle VALUE_GET_INT = bind("cr_value_get_int", fn(I, P, P, P));
    private static final MethodHandle VALUE_GET_FLOAT = bind("cr_value_get_float", fn(I, P, P, P));
    private static final MethodHandle VALUE_GET_DECIMAL = bind("cr_value_get_decimal", fn(I, P, P, P, P));
    private static final MethodHandle VALUE_GET_STRING = bind("cr_value_get_string", fn(I, P, P, P, P));
    private static final MethodHandle VALUE_GET_DATE = bind("cr_value_get_date", fn(I, P, P, P, P, P));
    private static final MethodHandle VALUE_GET_NAIVE_DATETIME =
            bind("cr_value_get_naive_datetime", fn(I, P, P, P, P, P, P, P, P, P));
    private static final MethodHandle VALUE_GET_AWARE_DATETIME =
            bind("cr_value_get_aware_datetime", fn(I, P, P, P, P, P, P, P));
    private static final MethodHandle VALUE_GET_OPAQUE_TOKEN = bind("cr_value_get_opaque_token", fn(I, P, P, P));
    private static final MethodHandle VALUE_LIST_GET = bind("cr_value_list_get", fn(P, P, SIZE_T));
    private static final MethodHandle VALUE_MAP_ENTRY = bind("cr_value_map_entry", fn(I, P, SIZE_T, P, P, P, P));

    // The C library's strlen, to read NUL-terminated texts with their exact
    // bounds; without it, cString scans byte by byte.
    private static final MethodHandle STRLEN = LOOKUP == null ? null : strlen();

    private static MethodHandle strlen() {
        int missing = MISSING.size();
        MethodHandle handle = bind(LINKER.defaultLookup(), "strlen", fn(SIZE_T, P));
        // Not a symbol of cleanrules: without it, cString scans byte by byte.
        while (MISSING.size() > missing) {
            MISSING.removeLast();
        }
        return handle;
    }

    /** Why the library cannot be used, or null. */
    private static final String PROBLEM;
    /** What made it unusable, or null. */
    private static final Throwable CAUSE;

    static {
        String problem = null;
        Throwable cause = LOAD_FAILURE != null ? LOAD_FAILURE : linkFailure;
        if (cause != null) {
            problem = cause.getMessage();
        } else if (!MISSING.isEmpty()) {
            problem = "the cleanrules library at " + where() + " lacks " + String.join(", ", MISSING)
                    + ": it is older than this binding; rebuild it (cargo build -p cleanrules-ffi)";
        } else {
            int abi = abiVersion();
            int format = formatVersion();
            if (abi != ABI_VERSION) {
                problem = "the cleanrules library at " + where() + " implements ABI version " + abi
                        + ", and this binding needs version " + ABI_VERSION
                        + ": use the library of the same release";
            } else if (format != CleanRules.FORMAT_VERSION) {
                problem = "the cleanrules library at " + where() + " reads format version " + format
                        + ", and this binding version " + CleanRules.FORMAT_VERSION
                        + ": use the library of the same release";
            }
        }
        PROBLEM = problem;
        CAUSE = cause;
    }

    private static String where() {
        return LOADED_FROM == null ? "the library path" : LOADED_FROM.toString();
    }

    /** Throw why the library cannot be used, if it cannot. */
    public static void ensureLoaded() {
        if (PROBLEM != null) {
            UnsatisfiedLinkError error = new UnsatisfiedLinkError(PROBLEM);
            if (CAUSE != null) {
                error.initCause(CAUSE);
            }
            throw error;
        }
    }

    /** Where the library was loaded from, when a path gave it. */
    public static Path loadedFrom() {
        return LOADED_FROM;
    }

    /**
     * The upcall stub of {@code target}, an {@code int32_t (*)(void *, void *)}
     * such as the actions' trampoline, for as long as the program runs. Call it
     * after {@link #ensureLoaded()}.
     */
    public static MemorySegment upcallStub(MethodHandle target) {
        return LINKER.upcallStub(target, fn(I, P, P), Arena.global());
    }

    /** The {@code CR_FORMAT_*} number of a format. */
    public static int format(Format format) {
        return switch (format) {
            case JSON -> 1;
            case YAML -> 2;
            case TEXT -> 3;
            case CSV -> 4;
        };
    }

    /** The truth of a {@code CR_TRUTH_*} number. */
    public static Truth truth(int code) {
        return switch (code) {
            case 1 -> Truth.TRUE;
            case 2 -> Truth.FALSE;
            case 3 -> Truth.UNKNOWN;
            default -> throw new IllegalStateException("cleanrules gave an unknown truth value " + code);
        };
    }

    /** The kind of a {@code CR_DOCUMENT_*} number. */
    public static DocumentKind documentKind(int code) {
        return switch (code) {
            case 1 -> DocumentKind.CONDITION;
            case 2 -> DocumentKind.RULE;
            case 3 -> DocumentKind.RULESET;
            case 4 -> DocumentKind.DECISION_TABLE;
            default -> throw new IllegalStateException("cleanrules gave an unknown document kind " + code);
        };
    }

    /** The core's default limits, in the order of {@code CR_LIMIT_*}. */
    public static long[] defaultLimits() {
        ensureLoaded();
        long[] values = new long[6];
        MemorySegment limits = limitsNew();
        try (Scope scope = new Scope()) {
            for (int i = 0; i < values.length; i++) {
                MemorySegment value = scope.size();
                scope.check(limitsGet(limits, i + 1, value, scope.error()));
                values[i] = value.get(JAVA_LONG, 0);
            }
        } finally {
            limitsFree(limits);
        }
        return values;
    }

    /** A Throwable from invokeExact: only unchecked ones are possible. */
    private static RuntimeException rethrow(Throwable thrown) {
        if (thrown instanceof RuntimeException unchecked) {
            throw unchecked;
        }
        if (thrown instanceof Error error) {
            throw error;
        }
        throw new IllegalStateException(thrown);
    }

    // ---- Texts.

    /** The text at {@code text}, NUL-terminated and UTF-8, or null for NULL. */
    public static String cString(MemorySegment text) {
        if (text.equals(MemorySegment.NULL)) {
            return null;
        }
        long length;
        if (STRLEN != null) {
            try {
                length = (long) STRLEN.invokeExact(text);
            } catch (Throwable thrown) {
                throw rethrow(thrown);
            }
        } else {
            // Without the C library's strlen, byte by byte: never past the NUL.
            length = 0;
            while (text.reinterpret(length + 1).get(JAVA_BYTE, length) != 0) {
                length++;
            }
        }
        return borrowed(text, length);
    }

    /** The {@code length} bytes at {@code text}, UTF-8. */
    public static String borrowed(MemorySegment text, long length) {
        if (length == 0) {
            return "";
        }
        return new String(text.reinterpret(length).toArray(JAVA_BYTE), StandardCharsets.UTF_8);
    }

    // ---- Version and errors.

    public static int abiVersion() {
        try {
            return (int) ABI.invokeExact();
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static String version() {
        try {
            return cString((MemorySegment) VERSION.invokeExact());
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int formatVersion() {
        try {
            return (int) FORMAT_VERSION.invokeExact();
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int errorCode(MemorySegment error) {
        try {
            return (int) ERROR_CODE.invokeExact(error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static String errorMessage(MemorySegment error) {
        try {
            return cString((MemorySegment) ERROR_MESSAGE.invokeExact(error));
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static String errorHint(MemorySegment error) {
        try {
            return cString((MemorySegment) ERROR_HINT.invokeExact(error));
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static String errorJson(MemorySegment error) {
        try {
            return cString((MemorySegment) ERROR_JSON.invokeExact(error));
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static MemorySegment errorDecision(MemorySegment error) {
        try {
            return (MemorySegment) ERROR_DECISION.invokeExact(error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void errorFree(MemorySegment error) {
        try {
            ERROR_FREE.invokeExact(error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void stringFree(MemorySegment text) {
        try {
            STRING_FREE.invokeExact(text);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    // ---- Limits.

    public static MemorySegment limitsNew() {
        try {
            return (MemorySegment) LIMITS_NEW.invokeExact();
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int limitsSet(MemorySegment limits, int limit, long value, MemorySegment error) {
        try {
            return (int) LIMITS_SET.invokeExact(limits, limit, value, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int limitsGet(MemorySegment limits, int limit, MemorySegment value, MemorySegment error) {
        try {
            return (int) LIMITS_GET.invokeExact(limits, limit, value, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void limitsFree(MemorySegment limits) {
        try {
            LIMITS_FREE.invokeExact(limits);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    // ---- Documents.

    public static int documentRead(
            MemorySegment text, long len, int format, MemorySegment limits, MemorySegment out, MemorySegment error) {
        try {
            return (int) DOCUMENT_READ.invokeExact(text, len, format, limits, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int documentKind(MemorySegment document) {
        try {
            return (int) DOCUMENT_KIND.invokeExact(document);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int documentWrite(
            MemorySegment document, int format, MemorySegment out, MemorySegment len, MemorySegment error) {
        try {
            return (int) DOCUMENT_WRITE.invokeExact(document, format, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void documentFree(MemorySegment document) {
        try {
            DOCUMENT_FREE.invokeExact(document);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    // ---- Rule sets.

    public static int rulesetRead(
            MemorySegment text, long len, int format, MemorySegment limits, MemorySegment out, MemorySegment error) {
        try {
            return (int) RULESET_READ.invokeExact(text, len, format, limits, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetFromDocument(MemorySegment document, MemorySegment out, MemorySegment error) {
        try {
            return (int) RULESET_FROM_DOCUMENT.invokeExact(document, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetWrite(
            MemorySegment ruleset, int format, MemorySegment out, MemorySegment len, MemorySegment error) {
        try {
            return (int) RULESET_WRITE.invokeExact(ruleset, format, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetSummary(
            MemorySegment ruleset, int format, MemorySegment out, MemorySegment len, MemorySegment error) {
        try {
            return (int) RULESET_SUMMARY.invokeExact(ruleset, format, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static long rulesetLen(MemorySegment ruleset) {
        try {
            return (long) RULESET_LEN.invokeExact(ruleset);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetRuleName(
            MemorySegment ruleset, long rule, MemorySegment name, MemorySegment len, MemorySegment error) {
        try {
            return (int) RULESET_RULE_NAME.invokeExact(ruleset, rule, name, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static String rulesetHitPolicy(MemorySegment ruleset) {
        try {
            return cString((MemorySegment) RULESET_HIT_POLICY.invokeExact(ruleset));
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static String rulesetAggregate(MemorySegment ruleset) {
        try {
            return cString((MemorySegment) RULESET_AGGREGATE.invokeExact(ruleset));
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static String rulesetLogic(MemorySegment ruleset) {
        try {
            return cString((MemorySegment) RULESET_LOGIC.invokeExact(ruleset));
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static boolean rulesetStrict(MemorySegment ruleset) {
        try {
            return (boolean) RULESET_STRICT.invokeExact(ruleset);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetFieldPaths(MemorySegment ruleset, MemorySegment out, MemorySegment error) {
        try {
            return (int) RULESET_FIELD_PATHS.invokeExact(ruleset, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetEvaluate(
            MemorySegment ruleset,
            MemorySegment input,
            long len,
            MemorySegment limits,
            MemorySegment out,
            MemorySegment error) {
        try {
            return (int) RULESET_EVALUATE.invokeExact(ruleset, input, len, limits, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetEvaluateValue(
            MemorySegment ruleset, MemorySegment input, MemorySegment limits, MemorySegment out, MemorySegment error) {
        try {
            return (int) RULESET_EVALUATE_VALUE.invokeExact(ruleset, input, limits, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetEvaluateMany(
            MemorySegment ruleset, MemorySegment inputs, MemorySegment limits, MemorySegment out, MemorySegment error) {
        try {
            return (int) RULESET_EVALUATE_MANY.invokeExact(ruleset, inputs, limits, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetReadDecision(
            MemorySegment ruleset,
            MemorySegment text,
            long len,
            int format,
            MemorySegment limits,
            MemorySegment out,
            MemorySegment error) {
        try {
            return (int) RULESET_READ_DECISION.invokeExact(ruleset, text, len, format, limits, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetCheckActions(MemorySegment ruleset, MemorySegment actions, MemorySegment error) {
        try {
            return (int) RULESET_CHECK_ACTIONS.invokeExact(ruleset, actions, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int rulesetRun(
            MemorySegment ruleset,
            MemorySegment actions,
            MemorySegment input,
            MemorySegment limits,
            MemorySegment out,
            MemorySegment error) {
        try {
            return (int) RULESET_RUN.invokeExact(ruleset, actions, input, limits, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void rulesetFree(MemorySegment ruleset) {
        try {
            RULESET_FREE.invokeExact(ruleset);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    // ---- Decisions and batches.

    public static long decisionLen(MemorySegment decision) {
        try {
            return (long) DECISION_LEN.invokeExact(decision);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionRuleName(
            MemorySegment decision, long rule, MemorySegment name, MemorySegment len, MemorySegment error) {
        try {
            return (int) DECISION_RULE_NAME.invokeExact(decision, rule, name, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionFired(MemorySegment decision, long rule, MemorySegment out, MemorySegment error) {
        try {
            return (int) DECISION_FIRED.invokeExact(decision, rule, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionTruth(MemorySegment decision, long rule, MemorySegment out, MemorySegment error) {
        try {
            return (int) DECISION_TRUTH.invokeExact(decision, rule, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static long decisionSelectedLen(MemorySegment decision) {
        try {
            return (long) DECISION_SELECTED_LEN.invokeExact(decision);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionSelected(MemorySegment decision, long index, MemorySegment rule, MemorySegment error) {
        try {
            return (int) DECISION_SELECTED.invokeExact(decision, index, rule, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionOutput(MemorySegment decision, MemorySegment out, MemorySegment len, MemorySegment error) {
        try {
            return (int) DECISION_OUTPUT.invokeExact(decision, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionOutputs(MemorySegment decision, MemorySegment out, MemorySegment len, MemorySegment error) {
        try {
            return (int) DECISION_OUTPUTS.invokeExact(decision, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionOutputValue(MemorySegment decision, MemorySegment out, MemorySegment error) {
        try {
            return (int) DECISION_OUTPUT_VALUE.invokeExact(decision, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionOutputsValue(MemorySegment decision, MemorySegment out, MemorySegment error) {
        try {
            return (int) DECISION_OUTPUTS_VALUE.invokeExact(decision, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionRuleOutput(MemorySegment decision, long rule, MemorySegment out, MemorySegment error) {
        try {
            return (int) DECISION_RULE_OUTPUT.invokeExact(decision, rule, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionWrite(
            MemorySegment decision, int format, MemorySegment out, MemorySegment len, MemorySegment error) {
        try {
            return (int) DECISION_WRITE.invokeExact(decision, format, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionExplain(
            MemorySegment decision, int format, MemorySegment out, MemorySegment len, MemorySegment error) {
        try {
            return (int) DECISION_EXPLAIN.invokeExact(decision, format, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int decisionClone(MemorySegment decision, MemorySegment out, MemorySegment error) {
        try {
            return (int) DECISION_CLONE.invokeExact(decision, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void decisionFree(MemorySegment decision) {
        try {
            DECISION_FREE.invokeExact(decision);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static long batchLen(MemorySegment batch) {
        try {
            return (long) BATCH_LEN.invokeExact(batch);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static MemorySegment batchGet(MemorySegment batch, long index) {
        try {
            return (MemorySegment) BATCH_GET.invokeExact(batch, index);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void batchFree(MemorySegment batch) {
        try {
            BATCH_FREE.invokeExact(batch);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    // ---- Nodes.

    public static int nodeRead(
            MemorySegment text, long len, int format, MemorySegment limits, MemorySegment out, MemorySegment error) {
        try {
            return (int) NODE_READ.invokeExact(text, len, format, limits, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int nodeWrite(MemorySegment node, int format, MemorySegment out, MemorySegment len, MemorySegment error) {
        try {
            return (int) NODE_WRITE.invokeExact(node, format, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int nodeFieldPaths(MemorySegment node, MemorySegment out, MemorySegment error) {
        try {
            return (int) NODE_FIELD_PATHS.invokeExact(node, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int nodeEvaluateTree(
            MemorySegment node,
            MemorySegment input,
            MemorySegment logic,
            long logicLen,
            boolean strict,
            MemorySegment out,
            MemorySegment len,
            MemorySegment error) {
        try {
            return (int) NODE_EVALUATE_TREE.invokeExact(node, input, logic, logicLen, strict, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int nodeEvaluateValueTree(
            MemorySegment node,
            MemorySegment input,
            boolean strict,
            MemorySegment out,
            MemorySegment len,
            MemorySegment error) {
        try {
            return (int) NODE_EVALUATE_VALUE_TREE.invokeExact(node, input, strict, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void nodeFree(MemorySegment node) {
        try {
            NODE_FREE.invokeExact(node);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    // ---- Actions.

    public static MemorySegment actionsNew() {
        try {
            return (MemorySegment) ACTIONS_NEW.invokeExact();
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int actionsAdd(
            MemorySegment actions,
            MemorySegment name,
            long len,
            MemorySegment action,
            MemorySegment data,
            MemorySegment error) {
        try {
            return (int) ACTIONS_ADD.invokeExact(actions, name, len, action, data, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void actionsFree(MemorySegment actions) {
        try {
            ACTIONS_FREE.invokeExact(actions);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static MemorySegment actionCallDecision(MemorySegment call) {
        try {
            return (MemorySegment) ACTION_CALL_DECISION.invokeExact(call);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static long actionCallRule(MemorySegment call) {
        try {
            return (long) ACTION_CALL_RULE.invokeExact(call);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void actionCallFail(MemorySegment call, MemorySegment message, long len) {
        try {
            ACTION_CALL_FAIL.invokeExact(call, message, len);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    // ---- Values.

    public static MemorySegment valueNewNull() {
        try {
            return (MemorySegment) VALUE_NEW_NULL.invokeExact();
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueNewNullReason(int reason, MemorySegment detail, long len, MemorySegment out, MemorySegment error) {
        try {
            return (int) VALUE_NEW_NULL_REASON.invokeExact(reason, detail, len, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static MemorySegment valueNewBool(boolean value) {
        try {
            return (MemorySegment) VALUE_NEW_BOOL.invokeExact(value);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static MemorySegment valueNewInt(long value) {
        try {
            return (MemorySegment) VALUE_NEW_INT.invokeExact(value);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static MemorySegment valueNewFloat(double value) {
        try {
            return (MemorySegment) VALUE_NEW_FLOAT.invokeExact(value);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueNewDecimal(MemorySegment text, long len, MemorySegment out, MemorySegment error) {
        try {
            return (int) VALUE_NEW_DECIMAL.invokeExact(text, len, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueNewString(MemorySegment text, long len, MemorySegment out, MemorySegment error) {
        try {
            return (int) VALUE_NEW_STRING.invokeExact(text, len, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueNewDate(int year, int month, int day, MemorySegment out, MemorySegment error) {
        try {
            return (int) VALUE_NEW_DATE.invokeExact(year, month, day, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueNewNaiveDatetime(
            int year,
            int month,
            int day,
            int hour,
            int minute,
            int second,
            int microsecond,
            MemorySegment out,
            MemorySegment error) {
        try {
            return (int) VALUE_NEW_NAIVE_DATETIME.invokeExact(
                    year, month, day, hour, minute, second, microsecond, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueNewAwareDatetime(
            long seconds,
            int microsecond,
            int offsetSeconds,
            MemorySegment zone,
            long len,
            MemorySegment out,
            MemorySegment error) {
        try {
            return (int) VALUE_NEW_AWARE_DATETIME.invokeExact(seconds, microsecond, offsetSeconds, zone, len, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static MemorySegment valueNewList(long len) {
        try {
            return (MemorySegment) VALUE_NEW_LIST.invokeExact(len);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static MemorySegment valueNewMap() {
        try {
            return (MemorySegment) VALUE_NEW_MAP.invokeExact();
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueNewOpaque(MemorySegment typeName, long len, long token, MemorySegment out, MemorySegment error) {
        try {
            return (int) VALUE_NEW_OPAQUE.invokeExact(typeName, len, token, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueListSet(MemorySegment list, long index, MemorySegment item, MemorySegment error) {
        try {
            return (int) VALUE_LIST_SET.invokeExact(list, index, item, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueMapInsert(MemorySegment map, MemorySegment key, long len, MemorySegment item, MemorySegment error) {
        try {
            return (int) VALUE_MAP_INSERT.invokeExact(map, key, len, item, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static void valueFree(MemorySegment value) {
        try {
            VALUE_FREE.invokeExact(value);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueFromJson(
            MemorySegment text, long len, MemorySegment limits, MemorySegment out, MemorySegment error) {
        try {
            return (int) VALUE_FROM_JSON.invokeExact(text, len, limits, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueKind(MemorySegment value) {
        try {
            return (int) VALUE_KIND.invokeExact(value);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static long valueLen(MemorySegment value) {
        try {
            return (long) VALUE_LEN.invokeExact(value);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueGetBool(MemorySegment value, MemorySegment out, MemorySegment error) {
        try {
            return (int) VALUE_GET_BOOL.invokeExact(value, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueGetInt(MemorySegment value, MemorySegment out, MemorySegment error) {
        try {
            return (int) VALUE_GET_INT.invokeExact(value, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueGetFloat(MemorySegment value, MemorySegment out, MemorySegment error) {
        try {
            return (int) VALUE_GET_FLOAT.invokeExact(value, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueGetDecimal(MemorySegment value, MemorySegment out, MemorySegment len, MemorySegment error) {
        try {
            return (int) VALUE_GET_DECIMAL.invokeExact(value, out, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueGetString(MemorySegment value, MemorySegment text, MemorySegment len, MemorySegment error) {
        try {
            return (int) VALUE_GET_STRING.invokeExact(value, text, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueGetDate(
            MemorySegment value, MemorySegment year, MemorySegment month, MemorySegment day, MemorySegment error) {
        try {
            return (int) VALUE_GET_DATE.invokeExact(value, year, month, day, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueGetNaiveDatetime(
            MemorySegment value,
            MemorySegment year,
            MemorySegment month,
            MemorySegment day,
            MemorySegment hour,
            MemorySegment minute,
            MemorySegment second,
            MemorySegment microsecond,
            MemorySegment error) {
        try {
            return (int) VALUE_GET_NAIVE_DATETIME.invokeExact(
                    value, year, month, day, hour, minute, second, microsecond, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueGetAwareDatetime(
            MemorySegment value,
            MemorySegment seconds,
            MemorySegment microsecond,
            MemorySegment offsetSeconds,
            MemorySegment zone,
            MemorySegment len,
            MemorySegment error) {
        try {
            return (int) VALUE_GET_AWARE_DATETIME.invokeExact(value, seconds, microsecond, offsetSeconds, zone, len, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueGetOpaqueToken(MemorySegment value, MemorySegment out, MemorySegment error) {
        try {
            return (int) VALUE_GET_OPAQUE_TOKEN.invokeExact(value, out, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static MemorySegment valueListGet(MemorySegment value, long index) {
        try {
            return (MemorySegment) VALUE_LIST_GET.invokeExact(value, index);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }

    public static int valueMapEntry(
            MemorySegment value,
            long index,
            MemorySegment key,
            MemorySegment len,
            MemorySegment item,
            MemorySegment error) {
        try {
            return (int) VALUE_MAP_ENTRY.invokeExact(value, index, key, len, item, error);
        } catch (Throwable thrown) {
            throw rethrow(thrown);
        }
    }
}
