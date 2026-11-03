/* A C program that uses the cleanrules package: read rules, evaluate. */

#include <cleanrules.h>

#include <stdbool.h>
#include <stdio.h>
#include <string.h>

int main(void) {
    const char *text = "RULE vip IF years > 5 THEN discount = 0.15";
    const char *input = "{\"years\": 7}";
    cr_ruleset *rules = NULL;
    cr_decision *decision = NULL;
    cr_error *error = NULL;
    if (cr_ruleset_read(text, strlen(text), CR_FORMAT_TEXT, NULL, &rules, &error) != CR_OK ||
        cr_ruleset_evaluate(rules, input, strlen(input), NULL, &decision, &error) != CR_OK) {
        fprintf(stderr, "%s\n", cr_error_message(error));
        cr_error_free(error);
        cr_ruleset_free(rules);
        return 1;
    }
    bool fired = false;
    if (cr_decision_fired(decision, 0, &fired, NULL) != CR_OK) {
        return 1;
    }
    printf("cleanrules %s from C: vip %s\n", cr_version(), fired ? "fired" : "did not fire");
    cr_decision_free(decision);
    cr_ruleset_free(rules);
    return 0;
}
