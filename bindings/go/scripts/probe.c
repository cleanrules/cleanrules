/*
 * The probe of a prebuilt static library for Linux (go-libs.yml): linked from
 * the library with the system libraries of its link_*.go file, in the oldest
 * environment the library supports (manylinux_2_28 for glibc, Alpine for
 * musl), and run there. check-libs.py then reads which C library and which
 * glibc symbol versions the program needs.
 */

#include <stdio.h>
#include <string.h>

#include "cleanrules.h"

static int fail(const char *what, cr_error *error) {
    fprintf(stderr, "probe: %s: %s\n", what, error ? cr_error_message(error) : "failed");
    cr_error_free(error);
    return 1;
}

int main(void) {
    if (cr_abi_version() != CR_ABI_VERSION) {
        fprintf(stderr, "probe: ABI version %u, header %d\n", cr_abi_version(), CR_ABI_VERSION);
        return 1;
    }
    const char *rules = "RULE vip IF customer.years > 5 THEN discount = 0.15";
    const char *input = "{\"customer\": {\"years\": 7}}";
    cr_error *error = NULL;
    cr_ruleset *ruleset = NULL;
    if (cr_ruleset_read(rules, strlen(rules), CR_FORMAT_TEXT, NULL, &ruleset, &error) != CR_OK) {
        return fail("reading the rules", error);
    }
    cr_decision *decision = NULL;
    if (cr_ruleset_evaluate(ruleset, input, strlen(input), NULL, &decision, &error) != CR_OK) {
        cr_ruleset_free(ruleset);
        return fail("evaluating", error);
    }
    char *json = NULL;
    size_t len = 0;
    if (cr_decision_write(decision, CR_FORMAT_JSON, &json, &len, &error) != CR_OK) {
        cr_decision_free(decision);
        cr_ruleset_free(ruleset);
        return fail("writing the decision", error);
    }
    int found = strstr(json, "\"selected\":[\"vip\"]") != NULL;
    printf("probe: cleanrules %s: %.*s\n", cr_version(), (int)len, json);
    cr_string_free(json);
    cr_decision_free(decision);
    cr_ruleset_free(ruleset);
    if (!found) {
        fprintf(stderr, "probe: vip was not selected\n");
        return 1;
    }
    return 0;
}
