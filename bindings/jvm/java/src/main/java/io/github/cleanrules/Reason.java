package io.github.cleanrules;

import java.util.List;
import java.util.Objects;
import java.util.Optional;

/** Why a result is unknown or null ({@code spec/ast.md}, <i>Results and reasons</i>). The engine makes these. */
public final class Reason {
    private final String code;
    private final List<List<Object>> paths;
    private final List<String> types;
    private final String detail;

    Reason(String code, List<List<Object>> paths, List<String> types, String detail) {
        this.code = code;
        this.paths = List.copyOf(paths);
        this.types = List.copyOf(types);
        this.detail = detail;
    }

    /**
     * The reason, such as {@code missing_field} or {@code type_mismatch}.
     *
     * @return the code
     */
    public String code() {
        return code;
    }

    /**
     * The field paths involved: lists of {@link String} keys and {@link Integer} indexes.
     *
     * @return the paths
     */
    public List<List<Object>> paths() {
        return paths;
    }

    /**
     * The type groups involved, for type mismatches.
     *
     * @return the type groups
     */
    public List<String> types() {
        return types;
    }

    /**
     * Informative text.
     *
     * @return the detail, or empty
     */
    public Optional<String> detail() {
        return Optional.ofNullable(detail);
    }

    @Override
    public boolean equals(Object other) {
        return other instanceof Reason reason
                && code.equals(reason.code)
                && paths.equals(reason.paths)
                && types.equals(reason.types)
                && Objects.equals(detail, reason.detail);
    }

    @Override
    public int hashCode() {
        return Objects.hash(code, paths, types, detail);
    }

    @Override
    public String toString() {
        return "Reason[" + code + ", paths=" + paths + ", types=" + types + ", detail=" + detail + "]";
    }
}
