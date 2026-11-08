package io.github.cleanrules.internal;

import static java.lang.foreign.ValueLayout.ADDRESS;

import io.github.cleanrules.EvaluationException;
import java.lang.foreign.MemorySegment;
import java.lang.reflect.Array;
import java.math.BigDecimal;
import java.math.BigInteger;
import java.nio.charset.StandardCharsets;
import java.time.Instant;
import java.time.LocalDate;
import java.time.LocalDateTime;
import java.time.OffsetDateTime;
import java.time.ZoneOffset;
import java.time.ZonedDateTime;
import java.util.ArrayList;
import java.util.Collection;
import java.util.IdentityHashMap;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.SequencedMap;
import java.util.TreeMap;

/**
 * Reading native Java data ({@code spec/data-model.md}, <i>Reading native
 * data</i> and <i>Java and Scala</i>) into the {@code cr_value} the C ABI
 * evaluates.
 *
 * <p>The reader walks the input along the paths that the rule set or node
 * reads, segment by segment, each object once, and converts only what it finds
 * at the end of each path: a sparse value tree, where a list keeps its length
 * and the positions no path reads are null. A string segment reads a key of a
 * {@link Map}, a component of a record, or a public field or accessor of
 * another object ({@link Members}); an integer segment indexes a {@link List}
 * or an array (but not a {@code byte[]}). Anything else at a segment is
 * converted as it is, and the core finds the path missing.
 *
 * <p>A getter that throws gives a null with reason {@code read_error}, whose
 * exception strict mode chains as the cause of its
 * {@link EvaluationException} ({@link Errors.Causes}). Objects with no
 * equivalent are opaque, with a token by identity, so that an output gives back
 * the very object.
 *
 * <p>What is read is counted against {@code max_values}
 * ({@code spec/data-model.md}, <i>Reading native data</i>, <i>Limits</i>):
 * each value placed in the tree other than the maps and lists walked through,
 * with everything inside it, and each position of a list walked through (its
 * declared length, checked before the list is allocated); an object met
 * through several references counts each time. Beyond the limit, reading
 * stops at once and the input is refused with the core's
 * {@code limit_exceeded} ({@code cr_error_new_max_values}).
 */
public final class DataReader implements AutoCloseable {
    /** How deep containers are converted whole: the format's default node depth. */
    private static final int MAX_DEPTH = 256;
    /** 0001-01-01T00:00:00Z and 9999-12-31T23:59:59Z, in seconds since the epoch. */
    private static final long FIRST_SECOND = -62135596800L;
    private static final long LAST_SECOND = 253402300799L;

    private enum Shape {
        SCALAR,
        LIST,
        ARRAY,
        MAP,
        MEMBERS,
        OPAQUE
    }

    private final Scope scope = new Scope();
    private final MemorySegment out = scope.pointer();
    private final IdentityHashMap<Object, Integer> tokens = new IdentityHashMap<>();
    private final IdentityHashMap<Object, Boolean> open = new IdentityHashMap<>();
    private final ArrayList<Object> at = new ArrayList<>();
    private int row = -1;
    /** The most values converted for one input. */
    private final long maxValues;
    /** How many more values the current input may convert. */
    private long left;
    /** The rule whose metadata or constant this reads, for the error; or null. */
    private final String rule;

    /** Thrown through the reader's own frames to stop reading an input beyond the limit. */
    private static final class Beyond extends RuntimeException {
        private static final long serialVersionUID = 1L;

        Beyond() {
            super("beyond max_values", null, false, false);
        }
    }

    private static final Beyond BEYOND = new Beyond();

    /** A reader under the default {@code maxValues}: for constants and metadata. */
    public DataReader() {
        this(null);
    }

    /** A reader of the metadata or a constant of {@code rule}, under the default {@code maxValues}. */
    public DataReader(String rule) {
        this(io.github.cleanrules.Limits.defaults().maxValues(), rule);
    }

    /** A reader within {@code limits}' {@code maxValues}, or the default's for null. */
    public static DataReader within(io.github.cleanrules.Limits limits) {
        return new DataReader((limits == null ? io.github.cleanrules.Limits.defaults() : limits).maxValues(), null);
    }

    private DataReader(long maxValues, String rule) {
        this.maxValues = maxValues;
        this.rule = rule;
    }

    /** The objects of the opaque values, by token, and the reads that threw. */
    public final Errors.Causes causes = new Errors.Causes(new ArrayList<>());

    /**
     * Read {@code data} along {@code tree}: an owned value.
     *
     * @throws io.github.cleanrules.FormatException with the code
     *     {@code limit_exceeded} beyond {@code maxValues}, with the row of a batch
     */
    public MemorySegment read(Object data, PathTree tree) {
        left = maxValues;
        try {
            return walk(data, tree);
        } catch (Beyond beyond) {
            throw tooMany();
        }
    }

    /** The core's {@code limit_exceeded} for this input, with its row and rule. */
    private RuntimeException tooMany() {
        MemorySegment name = MemorySegment.NULL;
        long length = 0;
        if (rule != null) {
            byte[] bytes = rule.getBytes(StandardCharsets.UTF_8);
            name = scope.scratch(bytes);
            length = bytes.length;
        }
        scope.check(Native.errorNewMaxValues(maxValues, row, name, length, out, scope.error()));
        return Errors.of(Native.CR_LIMIT_EXCEEDED, taken(), null);
    }

    /** Read each input of {@code inputs} along {@code tree}: an owned list. */
    public MemorySegment readMany(Iterable<?> inputs, PathTree tree) {
        List<Object> rows = new ArrayList<>();
        inputs.forEach(rows::add);
        MemorySegment list = list(rows.size());
        try {
            for (int index = 0; index < rows.size(); index++) {
                row = index;
                set(list, index, read(rows.get(index), tree));
            }
        } catch (RuntimeException | Error thrown) {
            Native.valueFree(list);
            throw thrown;
        } finally {
            row = -1;
        }
        return list;
    }

    @Override
    public void close() {
        scope.close();
    }

    // ---- Walking along paths.

    private MemorySegment walk(Object value, PathTree node) {
        value = unwrap(value);
        if (node.end || value == null) {
            return convert(value);
        }
        Shape shape = shape(value);
        return switch (shape) {
            case LIST, ARRAY -> walkList(value, shape, node);
            case MAP -> walkMap((Map<?, ?>) value, node);
            case MEMBERS -> walkMembers(value, node);
            default -> convert(value);
        };
    }

    private MemorySegment walkList(Object value, Shape shape, PathTree node) {
        int length;
        try {
            length = shape == Shape.ARRAY ? Array.getLength(value) : ((List<?>) value).size();
        } catch (RuntimeException thrown) {
            return placedError(thrown);
        }
        // The core holds a value for each position: counted before allocating.
        tick(length);
        MemorySegment list = list(length);
        try {
            for (Map.Entry<Integer, PathTree> step : node.indices.entrySet()) {
                int index = step.getKey();
                if (index >= length) {
                    continue;
                }
                at.add(index);
                try {
                    Object item;
                    try {
                        item = shape == Shape.ARRAY ? Array.get(value, index) : ((List<?>) value).get(index);
                    } catch (RuntimeException thrown) {
                        set(list, index, placedError(thrown));
                        continue;
                    }
                    set(list, index, walk(item, step.getValue()));
                } finally {
                    at.removeLast();
                }
            }
        } catch (RuntimeException | Error thrown) {
            Native.valueFree(list);
            throw thrown;
        }
        return list;
    }

    private MemorySegment walkMap(Map<?, ?> value, PathTree node) {
        MemorySegment map = map();
        try {
            for (Map.Entry<String, PathTree> step : node.keys.entrySet()) {
                String key = step.getKey();
                at.add(key);
                try {
                    Object item;
                    try {
                        if (!present(value, key)) {
                            continue;
                        }
                        item = value.get(key);
                    } catch (RuntimeException thrown) {
                        insert(map, key, placedError(thrown));
                        continue;
                    }
                    insert(map, key, walk(item, step.getValue()));
                } finally {
                    at.removeLast();
                }
            }
        } catch (RuntimeException | Error thrown) {
            Native.valueFree(map);
            throw thrown;
        }
        return map;
    }

    /** Whether {@code map} has {@code key}; a map whose keys cannot be strings has none. */
    private static boolean present(Map<?, ?> map, String key) {
        try {
            return map.containsKey(key);
        } catch (ClassCastException | NullPointerException notStringKeys) {
            return false;
        }
    }

    private MemorySegment walkMembers(Object value, PathTree node) {
        Map<String, Members.Member> members = Members.of(value.getClass());
        MemorySegment map = map();
        try {
            for (Map.Entry<String, PathTree> step : node.keys.entrySet()) {
                Members.Member member = members.get(step.getKey());
                if (member == null) {
                    continue;
                }
                at.add(step.getKey());
                try {
                    Object item;
                    try {
                        item = member.get(value);
                    } catch (VirtualMachineError fatal) {
                        throw fatal;
                    } catch (Throwable thrown) {
                        insert(map, step.getKey(), placedError(thrown));
                        continue;
                    }
                    insert(map, step.getKey(), walk(item, step.getValue()));
                } finally {
                    at.removeLast();
                }
            }
        } catch (RuntimeException | Error thrown) {
            Native.valueFree(map);
            throw thrown;
        }
        return map;
    }

    // ---- Whole values.

    private static Object unwrap(Object value) {
        while (value instanceof Optional<?> optional) {
            value = optional.orElse(null);
        }
        return value;
    }

    /** The packages of the binding (and of its Scala layer), whose objects are opaque as data. */
    private static final java.util.Set<String> BINDING = java.util.Set.of(
            "io.github.cleanrules",
            "io.github.cleanrules.ffi",
            "io.github.cleanrules.internal",
            "io.github.cleanrules.scaladsl");

    private static Shape shape(Object value) {
        Class<?> type = value.getClass();
        // The binding's own objects (rule sets, decisions, errors, limits...)
        // are never read through (spec/data-model.md, *Java and Scala*).
        if (BINDING.contains(type.getPackageName())) {
            return Shape.OPAQUE;
        }
        if (value instanceof Enum<?>
                || value instanceof Boolean
                || value instanceof Byte
                || value instanceof Short
                || value instanceof Integer
                || value instanceof Long
                || value instanceof BigInteger
                || value instanceof Float
                || value instanceof Double
                || value instanceof BigDecimal
                || value instanceof CharSequence
                || value instanceof LocalDate
                || value instanceof LocalDateTime
                || value instanceof OffsetDateTime
                || value instanceof ZonedDateTime
                || value instanceof Instant
                || value instanceof java.util.Date) {
            return Shape.SCALAR;
        }
        if (type.isArray()) {
            return type == byte[].class ? Shape.OPAQUE : Shape.ARRAY;
        }
        if (value instanceof List<?>) {
            return Shape.LIST;
        }
        if (value instanceof Map<?, ?>) {
            return Shape.MAP;
        }
        // Sets and other collections have no order; objects of the platform
        // (and those that expose the program: threads, class loaders, classes)
        // are never read through.
        if (value instanceof Collection<?>
                || value instanceof Thread
                || value instanceof ThreadGroup
                || value instanceof ClassLoader
                || value instanceof Iterator<?>
                || type.isHidden()
                || Members.platform(type)
                || SCALA_PRODUCT.get(type)) {
            return Shape.OPAQUE;
        }
        return Shape.MEMBERS;
    }

    /**
     * Whether a class implements Scala's {@code scala.Product} (a case class, a
     * tuple...), by the interface's name: the Scala layer reads case classes
     * by field name, and the Java reader never reads one as a JavaBean.
     */
    private static final ClassValue<Boolean> SCALA_PRODUCT = new ClassValue<>() {
        @Override
        protected Boolean computeValue(Class<?> type) {
            java.util.ArrayDeque<Class<?>> pending = new java.util.ArrayDeque<>();
            pending.add(type);
            while (!pending.isEmpty()) {
                Class<?> next = pending.removeFirst();
                if (next.isInterface() && next.getName().equals("scala.Product")) {
                    return true;
                }
                if (next.getSuperclass() != null) {
                    pending.add(next.getSuperclass());
                }
                pending.addAll(List.of(next.getInterfaces()));
            }
            return false;
        }
    };

    /** Count one value placed in the tree; stop reading beyond the limit. */
    private void tick() {
        tick(1);
    }

    /** Count {@code n} values about to be placed; stop reading beyond the limit. */
    private void tick(long n) {
        if (n > left) {
            throw BEYOND;
        }
        left -= n;
    }

    /** A read error placed in the tree, which counts as a value. */
    private MemorySegment placedError(Throwable thrown) {
        tick();
        return readError(thrown);
    }

    /** Refuse a list or map of {@code size} entries that cannot fit, before copying it. */
    private void fits(long size) {
        if (size > left) {
            throw BEYOND;
        }
    }

    /** The whole of {@code value}, where a path ends. */
    private MemorySegment convert(Object value) {
        tick();
        value = unwrap(value);
        if (value == null) {
            return Native.valueNewNull();
        }
        Shape shape = shape(value);
        if (shape == Shape.SCALAR) {
            return scalar(value);
        }
        if (shape == Shape.OPAQUE || open.containsKey(value) || open.size() >= MAX_DEPTH) {
            return opaque(value);
        }
        open.put(value, Boolean.TRUE);
        try {
            return switch (shape) {
                case LIST -> wholeList((List<?>) value);
                case ARRAY -> wholeArray(value);
                case MAP -> wholeMap((Map<?, ?>) value);
                default -> wholeMembers(value);
            };
        } finally {
            open.remove(value);
        }
    }

    private MemorySegment wholeList(List<?> value) {
        List<Object> items;
        int size;
        try {
            size = value.size();
        } catch (RuntimeException thrown) {
            return readError(thrown);
        }
        // Each element counts as it is converted: a size beyond what is left
        // is refused before the list is copied.
        fits(size);
        try {
            items = new ArrayList<>(value);
        } catch (RuntimeException thrown) {
            return readError(thrown);
        }
        MemorySegment list = list(items.size());
        try {
            for (int i = 0; i < items.size(); i++) {
                set(list, i, convert(items.get(i)));
            }
        } catch (RuntimeException | Error thrown) {
            Native.valueFree(list);
            throw thrown;
        }
        return list;
    }

    private MemorySegment wholeArray(Object value) {
        int length = Array.getLength(value);
        fits(length);
        MemorySegment list = list(length);
        try {
            for (int i = 0; i < length; i++) {
                set(list, i, convert(Array.get(value, i)));
            }
        } catch (RuntimeException | Error thrown) {
            Native.valueFree(list);
            throw thrown;
        }
        return list;
    }

    private MemorySegment wholeMap(Map<?, ?> value) {
        // Maps with an order keep it; the others are sorted by key, by code
        // point (spec/data-model.md, *Lists and maps*). Keys that are not
        // strings, or not Unicode, are left out.
        Map<String, Object> entries = value instanceof SequencedMap<?, ?>
                ? new java.util.LinkedHashMap<>()
                : new TreeMap<>(Members::compareCodePoints);
        try {
            for (Map.Entry<?, ?> entry : value.entrySet()) {
                if (entry.getKey() instanceof String key && Text.wellFormed(key)) {
                    entries.put(key, entry.getValue());
                }
            }
        } catch (RuntimeException thrown) {
            return readError(thrown);
        }
        MemorySegment map = map();
        try {
            for (Map.Entry<String, Object> entry : entries.entrySet()) {
                insert(map, entry.getKey(), convert(entry.getValue()));
            }
        } catch (RuntimeException | Error thrown) {
            Native.valueFree(map);
            throw thrown;
        }
        return map;
    }

    private MemorySegment wholeMembers(Object value) {
        MemorySegment map = map();
        try {
            for (Map.Entry<String, Members.Member> entry : Members.of(value.getClass()).entrySet()) {
                Object item;
                try {
                    item = entry.getValue().get(value);
                } catch (VirtualMachineError fatal) {
                    throw fatal;
                } catch (Throwable thrown) {
                    tick();
                    insert(map, entry.getKey(), readError(thrown));
                    continue;
                }
                insert(map, entry.getKey(), convert(item));
            }
        } catch (RuntimeException | Error thrown) {
            Native.valueFree(map);
            throw thrown;
        }
        return map;
    }

    private MemorySegment scalar(Object value) {
        return switch (value) {
            case Boolean bool -> Native.valueNewBool(bool);
            case Byte number -> Native.valueNewInt(number);
            case Short number -> Native.valueNewInt(number);
            case Integer number -> Native.valueNewInt(number);
            case Long number -> Native.valueNewInt(number);
            case BigInteger number -> number.bitLength() < 64
                    ? Native.valueNewInt(number.longValue())
                    : nullWith(Native.CR_REASON_INT_OVERFLOW, "");
            case Float number -> number.isNaN() ? Native.valueNewNull() : Native.valueNewFloat(number.doubleValue());
            case Double number -> number.isNaN() ? Native.valueNewNull() : Native.valueNewFloat(number);
            case BigDecimal number -> decimal(number);
            case String text -> string(text, value);
            // An enum constant is its name, also one with a body or of the platform.
            case Enum<?> constant -> string(constant.name(), value);
            case CharSequence text -> {
                String string;
                try {
                    string = text.toString();
                } catch (RuntimeException thrown) {
                    yield readError(thrown);
                }
                yield string(string, value);
            }
            case LocalDate date -> date(date, value);
            case LocalDateTime local -> naive(local, value);
            case Instant instant -> aware(instant.getEpochSecond(), instant.getNano(), 0, null, value);
            case OffsetDateTime aware ->
                aware(aware.toEpochSecond(), aware.getNano(), aware.getOffset().getTotalSeconds(), null, value);
            case ZonedDateTime aware -> aware(
                    aware.toEpochSecond(),
                    aware.getNano(),
                    aware.getOffset().getTotalSeconds(),
                    aware.getZone() instanceof ZoneOffset ? null : aware.getZone().getId(),
                    value);
            case java.util.Date date -> legacyDate(date);
            default -> opaque(value);
        };
    }

    /** A {@code java.util.Date}, a {@code java.sql.Date} or a {@code java.sql.Timestamp}. */
    @SuppressWarnings("deprecation") // java.sql.Date's day is its deprecated local fields.
    private MemorySegment legacyDate(java.util.Date date) {
        try {
            return switch (date.getClass().getName()) {
                // A day, in the default time zone, as java.sql.Date.toLocalDate() reads it.
                case "java.sql.Date" -> date(
                        LocalDate.of(date.getYear() + 1900, date.getMonth() + 1, date.getDate()), date);
                // A time of day without a date.
                case "java.sql.Time" -> opaque(date);
                default -> {
                    Instant instant = date.toInstant();
                    yield aware(instant.getEpochSecond(), instant.getNano(), 0, null, date);
                }
            };
        } catch (RuntimeException thrown) {
            return readError(thrown);
        }
    }

    private MemorySegment decimal(BigDecimal number) {
        byte[] text = number.toString().getBytes(StandardCharsets.US_ASCII);
        int status = Native.valueNewDecimal(scope.scratch(text), text.length, out, scope.error());
        return status == Native.CR_OK ? taken() : refused(status, number);
    }

    private MemorySegment string(String text, Object value) {
        // A lone surrogate has no Unicode form (spec/data-model.md, *Opaque values*).
        if (!Text.wellFormed(text)) {
            return opaque(value);
        }
        byte[] bytes = text.getBytes(StandardCharsets.UTF_8);
        scope.check(Native.valueNewString(scope.scratch(bytes), bytes.length, out, scope.error()));
        return taken();
    }

    private MemorySegment date(LocalDate date, Object value) {
        if (date.getYear() < 1 || date.getYear() > 9999) {
            return opaque(value);
        }
        int status = Native.valueNewDate(
                date.getYear(), date.getMonthValue(), date.getDayOfMonth(), out, scope.error());
        return status == Native.CR_OK ? taken() : refused(status, value);
    }

    private MemorySegment naive(LocalDateTime local, Object value) {
        if (local.getYear() < 1 || local.getYear() > 9999) {
            return opaque(value);
        }
        // Nanoseconds are truncated toward the past to the microsecond.
        int status = Native.valueNewNaiveDatetime(
                local.getYear(),
                local.getMonthValue(),
                local.getDayOfMonth(),
                local.getHour(),
                local.getMinute(),
                local.getSecond(),
                local.getNano() / 1000,
                out,
                scope.error());
        return status == Native.CR_OK ? taken() : refused(status, value);
    }

    /**
     * An aware datetime, if its UTC instant and its local date and time fall
     * within years 1 to 9999 (spec/data-model.md, *Datetimes*); opaque otherwise,
     * and also when the core refuses it.
     */
    private MemorySegment aware(long seconds, int nanos, int offset, String zone, Object value) {
        long local = seconds + offset;
        if (seconds < FIRST_SECOND || seconds > LAST_SECOND || local < FIRST_SECOND || local > LAST_SECOND) {
            return opaque(value);
        }
        byte[] name = zone == null ? new byte[0] : zone.getBytes(StandardCharsets.UTF_8);
        int status = Native.valueNewAwareDatetime(
                seconds, nanos / 1000, offset, scope.scratch(name), name.length, out, scope.error());
        return status == Native.CR_OK ? taken() : refused(status, value);
    }

    /**
     * What the core refused to build from a native value is opaque, so that the
     * object comes back; the error is freed.
     */
    private MemorySegment refused(int status, Object value) {
        scope.failure(status);
        return opaque(value);
    }

    // ---- Building values.

    private MemorySegment taken() {
        return out.get(ADDRESS, 0);
    }

    /** The token of {@code value}: its position among the objects met, given once. */
    private int token(Object value) {
        Integer token = tokens.get(value);
        if (token == null) {
            token = causes.opaques.size();
            causes.opaques.add(value);
            tokens.put(value, token);
        }
        return token;
    }

    private MemorySegment opaque(Object value) {
        byte[] name = value.getClass().getName().getBytes(StandardCharsets.UTF_8);
        scope.check(Native.valueNewOpaque(scope.scratch(name), name.length, token(value), out, scope.error()));
        return taken();
    }

    private MemorySegment nullWith(int reason, String detail) {
        byte[] bytes = Text.document(detail);
        scope.check(Native.valueNewNullReason(reason, scope.scratch(bytes), bytes.length, out, scope.error()));
        return taken();
    }

    /** A null with reason {@code read_error}, whose exception strict mode chains. */
    private MemorySegment readError(Throwable thrown) {
        causes.reads.add(new Errors.ReadError(row, List.copyOf(at), thrown));
        return nullWith(Native.CR_REASON_READ_ERROR, describe(thrown));
    }

    /** What a read error says of the exception. */
    public static String describe(Throwable thrown) {
        try {
            return thrown.toString();
        } catch (RuntimeException failed) {
            return thrown.getClass().getName();
        }
    }

    private static MemorySegment list(long length) {
        MemorySegment list = Native.valueNewList(length);
        if (list.equals(MemorySegment.NULL)) {
            throw new OutOfMemoryError("cannot allocate a list of " + length + " values");
        }
        return list;
    }

    private static MemorySegment map() {
        return Native.valueNewMap();
    }

    /** Move {@code item} into {@code list} at {@code index}. */
    private void set(MemorySegment list, long index, MemorySegment item) {
        scope.check(Native.valueListSet(list, index, item, scope.error()));
    }

    /** Move {@code item} into {@code map} under {@code key}. */
    private void insert(MemorySegment map, String key, MemorySegment item) {
        byte[] bytes = key.getBytes(StandardCharsets.UTF_8);
        scope.check(Native.valueMapInsert(map, scope.scratch(bytes), bytes.length, item, scope.error()));
    }
}
