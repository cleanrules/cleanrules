package io.github.cleanrules.ffi;

import static java.lang.foreign.ValueLayout.ADDRESS;

import io.github.cleanrules.CleanRulesException;
import io.github.cleanrules.DocumentKind;
import io.github.cleanrules.ExpressionException;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.Limits;
import io.github.cleanrules.RuleSyntaxException;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.internal.Handle;
import io.github.cleanrules.internal.Native;
import io.github.cleanrules.internal.Scope;
import io.github.cleanrules.internal.Text;
import java.lang.foreign.MemorySegment;
import java.util.Objects;

/**
 * A document read in any format: a condition, a rule, a rule set or a decision
 * table ({@code spec/json-format.md}, <i>Documents</i>). A rule set is made
 * from one with {@link RuleSet#fromDocument(Document)}.
 *
 * <p>A document owns native memory: close it when done (it is
 * {@link AutoCloseable}), or the cleaner frees it once it is unreachable.
 * Closing twice does nothing, and using a closed document is an
 * {@link IllegalStateException}. Documents never change, so they may be used
 * from several threads at once; closing one while another thread uses it
 * frees it when that use ends.
 */
public final class Document implements AutoCloseable {
    private final Handle handle;

    private Document(MemorySegment pointer) {
        handle = Handle.own(this, pointer, Native::documentFree, "Document");
    }

    /**
     * Read a document.
     *
     * @param text the document's text
     * @param format its syntax: JSON, YAML, the rule syntax, or CSV for a decision table
     * @return the document
     * @throws FormatException if the text is not a valid document, or beyond the default limits
     * @throws CleanRulesException for other errors of the document, such as a
     *     {@link RuleSyntaxException} or an {@link ExpressionException}
     */
    public static Document read(String text, Format format) {
        return read(text, format, null);
    }

    /**
     * Read a document within limits.
     *
     * @param text the document's text
     * @param format its syntax
     * @param limits the limits, or null for the defaults
     * @return the document
     * @throws CleanRulesException if the document is invalid
     */
    public static Document read(String text, Format format, Limits limits) {
        return read(Text.document(Objects.requireNonNull(text, "text")), format, limits);
    }

    /**
     * Read a document from UTF-8 bytes; bytes that are not UTF-8 are a
     * {@link FormatException} at the line and column of the first invalid one.
     *
     * @param text the document's bytes
     * @param format its syntax
     * @return the document
     * @throws CleanRulesException if the document is invalid
     */
    public static Document read(byte[] text, Format format) {
        return read(text, format, null);
    }

    /**
     * Read a document from UTF-8 bytes within limits.
     *
     * @param text the document's bytes
     * @param format its syntax
     * @param limits the limits, or null for the defaults
     * @return the document
     * @throws CleanRulesException if the document is invalid
     */
    public static Document read(byte[] text, Format format, Limits limits) {
        Objects.requireNonNull(text, "text");
        Objects.requireNonNull(format, "format");
        Native.ensureLoaded();
        try (Scope scope = new Scope()) {
            MemorySegment out = scope.pointer();
            scope.check(Native.documentRead(
                    scope.bytes(text), text.length, Native.format(format), scope.limits(limits), out, scope.error()));
            return new Document(out.get(ADDRESS, 0));
        }
    }

    /**
     * What the document holds.
     *
     * @return its kind
     */
    public DocumentKind kind() {
        MemorySegment pointer = handle.acquire();
        try {
            return Native.documentKind(Native.documentKind(pointer));
        } finally {
            handle.release();
        }
    }

    /**
     * Write the document as JSON, in canonical form.
     *
     * @return the JSON text
     */
    public String write() {
        return write(Format.JSON);
    }

    /**
     * Write the document in a format, in canonical form. CSV holds decision
     * tables only; the rule syntax holds no decision tables, metadata or NaN.
     *
     * @param format the syntax
     * @return the text
     * @throws UnsupportedException if the format cannot hold this document
     */
    public String write(Format format) {
        Objects.requireNonNull(format, "format");
        MemorySegment pointer = handle.acquire();
        try {
            return Scope.text((out, len, error) -> Native.documentWrite(pointer, Native.format(format), out, len, error));
        } finally {
            handle.release();
        }
    }

    /** Hold the native document for one call; {@link #release()} it after. */
    MemorySegment acquire() {
        return handle.acquire();
    }

    void release() {
        handle.release();
    }

    /** Free the native document now; closing again does nothing. */
    @Override
    public void close() {
        handle.closeFromOwner();
    }
}
