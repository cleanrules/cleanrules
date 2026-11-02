/*
 * The rules of discounts.rules, from C: read them, evaluate one input, print
 * the output and the explanation, and report an error.
 *
 *     cargo build -p cleanrules-ffi
 *     cc -I crates/ffi/include docs/examples/discounts.c \
 *        target/debug/libcleanrules_ffi.a -lm -lpthread -ldl -o discounts
 *     ./discounts docs/examples/discounts.rules
 *
 * (On macOS, link with -lm alone.)
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "cleanrules.h"

/* Print an error and free it. */
static int fail(const char *what, cr_error *error) {
    fprintf(stderr, "%s: %s (%s)\n", what, cr_error_message(error),
            cr_status_name(cr_error_code(error)));
    if (cr_error_hint(error) != NULL) {
        fprintf(stderr, "hint: %s\n", cr_error_hint(error));
    }
    cr_error_free(error);
    return 1;
}

/* The whole file, or NULL. */
static char *slurp(const char *path, size_t *len) {
    FILE *file = fopen(path, "rb");
    if (file == NULL) {
        return NULL;
    }
    long size = -1;
    if (fseek(file, 0, SEEK_END) == 0) {
        size = ftell(file);
    }
    char *text = NULL;
    if (size >= 0 && fseek(file, 0, SEEK_SET) == 0) {
        text = malloc(size > 0 ? (size_t)size : 1);
    }
    if (text != NULL) {
        *len = fread(text, 1, (size_t)size, file);
    }
    fclose(file);
    return text;
}

int main(int argc, char **argv) {
    if (cr_abi_version() != CR_ABI_VERSION) {
        fprintf(stderr, "this program was built for another cleanrules ABI\n");
        return 1;
    }
    if (argc != 2) {
        fprintf(stderr, "usage: %s discounts.rules\n", argv[0]);
        return 2;
    }
    size_t len = 0;
    char *text = slurp(argv[1], &len);
    if (text == NULL) {
        perror(argv[1]);
        return 1;
    }

    cr_error *error = NULL;
    cr_ruleset *rules = NULL;
    cr_status status = cr_ruleset_read(text, len, CR_FORMAT_TEXT, NULL, &rules, &error);
    free(text);
    if (status != CR_OK) {
        return fail("reading the rules", error);
    }

    const char *input = "{\"customer\": {\"years\": 7}, \"order\": {\"total\": 1500}}";
    cr_decision *decision = NULL;
    if (cr_ruleset_evaluate(rules, input, strlen(input), NULL, &decision, &error) != CR_OK) {
        cr_ruleset_free(rules);
        return fail("evaluating", error);
    }

    char *output = NULL;
    char *explanation = NULL;
    if (cr_decision_output(decision, &output, NULL, &error) != CR_OK ||
        cr_decision_explain(decision, CR_FORMAT_TEXT, &explanation, NULL, &error) != CR_OK) {
        cr_string_free(output); /* NULL when cr_decision_output failed */
        cr_decision_free(decision);
        cr_ruleset_free(rules);
        return fail("reading the decision", error);
    }
    printf("output: %s\n\n%s\n", output, explanation);
    cr_string_free(output);
    cr_string_free(explanation);
    cr_decision_free(decision);

    /* An input that is not JSON: the error says where, in its JSON form. */
    const char *broken = "{\"customer\": ";
    status = cr_ruleset_evaluate(rules, broken, strlen(broken), NULL, &decision, &error);
    cr_ruleset_free(rules);
    if (status == CR_OK) {
        cr_decision_free(decision);
        return 1;
    }
    printf("\nerror: %s\n", cr_error_json(error));
    int expected = cr_error_is(status, CR_FORMAT_ERROR);
    cr_error_free(error);
    return expected ? 0 : 1;
}
