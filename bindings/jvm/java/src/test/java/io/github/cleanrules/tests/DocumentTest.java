package io.github.cleanrules.tests;

import static org.junit.jupiter.api.Assertions.assertArrayEquals;
import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertThrows;
import static org.junit.jupiter.api.Assertions.assertTrue;

import io.github.cleanrules.DocumentKind;
import io.github.cleanrules.Format;
import io.github.cleanrules.FormatException;
import io.github.cleanrules.UnsupportedException;
import io.github.cleanrules.ffi.Decision;
import io.github.cleanrules.ffi.Document;
import io.github.cleanrules.ffi.RuleSet;
import io.github.cleanrules.internal.Text;
import java.nio.charset.StandardCharsets;
import java.util.OptionalInt;
import org.junit.jupiter.api.Test;

class DocumentTest {
    @Test
    void aDocumentIsReadAndWrittenInEveryFormat() {
        try (Document text = Document.read(Fixtures.DISCOUNTS, Format.TEXT)) {
            assertEquals(DocumentKind.RULESET, text.kind());
            String json = text.write();
            assertTrue(json.startsWith("{\"version\":1,\"ruleset\":"), json);
            try (Document again = Document.read(json, Format.JSON);
                    Document yaml = Document.read(again.write(Format.YAML), Format.YAML)) {
                assertEquals(json, again.write(Format.JSON));
                assertEquals(json, yaml.write());
                assertEquals(text.write(Format.TEXT), yaml.write(Format.TEXT));
            }
        }
        try (Document condition = Document.read("customer.years > 5", Format.TEXT);
                Document rule = Document.read("IF a > 1 THEN x = 1", Format.TEXT)) {
            assertEquals(DocumentKind.CONDITION, condition.kind());
            assertEquals(DocumentKind.RULE, rule.kind());
            assertEquals("customer.years > 5", condition.write(Format.TEXT));
        }
    }

    @Test
    void decisionTablesAreReadFromCsvAndBecomeRuleSets() {
        String csv = """
                RULE,customer.years,THEN discount
                vip,> 5,0.15
                other,,0
                """;
        try (Document table = Document.read(csv, Format.CSV);
                RuleSet rules = RuleSet.fromDocument(table);
                Decision decision = rules.evaluateJson("{\"customer\": {\"years\": 7}}")) {
            assertEquals(DocumentKind.DECISION_TABLE, table.kind());
            assertTrue(table.write(Format.CSV).contains("RULE,customer.years,THEN discount"), table.write(Format.CSV));
            assertEquals(2, rules.size());
            assertEquals("vip", rules.ruleName(0));
            assertEquals(java.util.List.of(0), decision.selected());
        }
    }

    @Test
    void aConditionIsNotARuleSet() {
        try (Document condition = Document.read("a > 1", Format.TEXT)) {
            FormatException error = assertThrows(FormatException.class, () -> RuleSet.fromDocument(condition));
            assertEquals("format_error", error.code());
        }
    }

    @Test
    void writingInAFormatThatCannotHoldItIsUnsupported() {
        try (Document rules = Document.read(Fixtures.DISCOUNTS, Format.TEXT)) {
            UnsupportedException error = assertThrows(UnsupportedException.class, () -> rules.write(Format.CSV));
            assertEquals("unsupported", error.code());
            assertTrue(error.operation().isPresent());
            assertTrue(error.reason().isPresent());
        }
    }

    @Test
    void bytesAreReadAsUtf8WithThePlaceOfTheFirstInvalidByte() {
        byte[] good = "a > 1".getBytes(StandardCharsets.UTF_8);
        try (Document document = Document.read(good, Format.TEXT)) {
            assertEquals(DocumentKind.CONDITION, document.kind());
        }
        byte[] bad = {'a', ' ', '>', ' ', '\n', 'x', (byte) 0xFF};
        FormatException error = assertThrows(FormatException.class, () -> Document.read(bad, Format.TEXT));
        assertEquals(OptionalInt.of(2), error.line());
        assertEquals(OptionalInt.of(2), error.column());
    }

    @Test
    void aLoneSurrogateIsAFormatErrorNotAReplacementCharacter() {
        FormatException error =
                assertThrows(FormatException.class, () -> Document.read("name = \"a\uD800\"", Format.TEXT));
        assertEquals("format_error", error.code());
        assertEquals(OptionalInt.of(1), error.line());
        assertEquals(OptionalInt.of(10), error.column());
        // A surrogate pair is fine.
        try (Document pair = Document.read("name = \"😀\"", Format.TEXT)) {
            assertTrue(pair.write(Format.TEXT).contains("😀"));
        }
        assertArrayEquals(new byte[] {(byte) 0xED, (byte) 0xA0, (byte) 0x80}, Text.document("\uD800"));
    }

    @Test
    void anEmptyTextIsReadToo() {
        FormatException error = assertThrows(FormatException.class, () -> Document.read(new byte[0], Format.JSON));
        assertEquals("format_error", error.code());
    }
}
