package io.github.cleanrules.internal;

import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;

/** Java strings as the UTF-8 the C ABI takes. */
public final class Text {
    private Text() {}

    /** Whether {@code text} has no lone surrogate, so that it has a UTF-8 form. */
    public static boolean wellFormed(String text) {
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            if (Character.isHighSurrogate(c)) {
                if (i + 1 < text.length() && Character.isLowSurrogate(text.charAt(i + 1))) {
                    i++;
                    continue;
                }
                return false;
            }
            if (Character.isLowSurrogate(c)) {
                return false;
            }
        }
        return true;
    }

    /**
     * The UTF-8 of a document's text. A lone surrogate, which UTF-8 cannot
     * hold, is written as its three-byte form (as CESU-8 would), which is not
     * UTF-8: the core then reports a {@code format_error} at its line and
     * column, as for any text that is not UTF-8, instead of reading a
     * replacement character in silence.
     */
    public static byte[] document(String text) {
        if (wellFormed(text)) {
            return text.getBytes(StandardCharsets.UTF_8);
        }
        ByteArrayOutputStream out = new ByteArrayOutputStream(text.length() + 8);
        int start = 0;
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            boolean pair = Character.isHighSurrogate(c)
                    && i + 1 < text.length()
                    && Character.isLowSurrogate(text.charAt(i + 1));
            if (pair) {
                i++;
            } else if (Character.isSurrogate(c)) {
                out.writeBytes(text.substring(start, i).getBytes(StandardCharsets.UTF_8));
                out.write(0xE0 | (c >> 12));
                out.write(0x80 | ((c >> 6) & 0x3F));
                out.write(0x80 | (c & 0x3F));
                start = i + 1;
            }
        }
        out.writeBytes(text.substring(start).getBytes(StandardCharsets.UTF_8));
        return out.toByteArray();
    }

    /** The UTF-8 of a name the C ABI checks (an action's, a logic's). */
    public static byte[] name(String text) {
        return document(text);
    }
}
