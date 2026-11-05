/*
 * The rules of discounts.rules, from C: read them, evaluate one input given as
 * JSON and then built natively, print the output, the explanation and which
 * rules fired, run the rules' named action (notify_sales) as a C function,
 * report an error, and build the vip rule in code with the constructors.
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

/* Move an int into a map under a key. */
static cr_status put_int(cr_value *map, const char *key, int64_t value, cr_error **error) {
    return cr_value_map_insert(map, key, strlen(key), cr_value_new_int(value), error);
}

/* The input of main, built natively: {customer: {years: 7}, order: {total: 1500}}. */
static cr_value *native_input(cr_error **error) {
    cr_value *input = cr_value_new_map();
    cr_value *customer = cr_value_new_map();
    cr_value *order = cr_value_new_map();
    if (put_int(customer, "years", 7, error) != CR_OK ||
        put_int(order, "total", 1500, error) != CR_OK) {
        cr_value_free(customer);
        cr_value_free(order);
        cr_value_free(input);
        return NULL;
    }
    /* Each child moves into its parent, even when inserting it fails. */
    if (cr_value_map_insert(input, "customer", 8, customer, error) != CR_OK ||
        cr_value_map_insert(input, "order", 5, order, error) != CR_OK) {
        cr_value_free(input);
        return NULL;
    }
    return input;
}

/* Print which rules fired and the discount, read without JSON. */
static int show(const cr_ruleset *rules, const cr_decision *decision) {
    for (size_t i = 0; i < cr_decision_len(decision); i++) {
        const char *name = NULL;
        size_t len = 0;
        bool fired = false;
        if (cr_ruleset_rule_name(rules, i, &name, &len, NULL) != CR_OK ||
            cr_decision_fired(decision, i, &fired, NULL) != CR_OK) {
            return 1;
        }
        printf("%.*s %s\n", (int)len, name, fired ? "fired" : "did not fire");
    }
    cr_error *error = NULL;
    cr_value *output = NULL;
    if (cr_decision_output_value(decision, &output, &error) != CR_OK) {
        return fail("reading the output", error);
    }
    char *discount = NULL;
    const cr_value *value = cr_value_map_get(output, "discount", 8);
    if (cr_value_get_decimal(value, &discount, NULL, NULL) == CR_OK) {
        printf("discount: %s\n", discount);
    }
    cr_string_free(discount);
    cr_value_free(output);
    return 0;
}

/* The action vip names (DO notify_sales): called by cr_ruleset_run for each
 * selected rule that names it. Returning anything but 0 stops the run, after
 * cr_action_call_fail says why. */
static int32_t notify_sales(void *data, cr_action_call *call) {
    const char *name = NULL;
    size_t len = 0;
    if (cr_decision_rule_name(cr_action_call_decision(call), cr_action_call_rule(call), &name,
                              &len, NULL) != CR_OK) {
        cr_action_call_fail(call, "no rule name", 12);
        return 1;
    }
    int *calls = data;
    *calls += 1;
    printf("notify_sales: rule %.*s was selected\n", (int)len, name);
    return 0;
}

/* Move a string into a list at a position. */
static cr_status set_string(cr_value *list, size_t at, const char *text, cr_error **error) {
    cr_value *item = NULL;
    cr_status status = cr_value_new_string(text, strlen(text), &item, error);
    return status != CR_OK ? status : cr_value_list_set(list, at, item, error);
}

/*
 * The vip rule built in code: customer.years > 5, with the payload discount = 5
 * and priority 10. The core checks each step; every argument is copied, so
 * each node, value and rule is freed by whoever made it.
 */
static int built_in_code(void) {
    cr_error *error = NULL;
    cr_value *path = cr_value_new_list(2);
    cr_value *five = cr_value_new_int(5);
    cr_node *years = NULL, *limit = NULL, *when = NULL;
    cr_rule *named = NULL, *paid = NULL, *vip = NULL;
    cr_ruleset *built = NULL;
    char *text = NULL;
    int failed = set_string(path, 0, "customer", &error) != CR_OK ||
                 set_string(path, 1, "years", &error) != CR_OK ||
                 cr_node_new_field(path, &years, &error) != CR_OK ||
                 cr_node_new_constant(five, &limit, &error) != CR_OK ||
                 cr_node_new_compare("gt", 2, years, limit, &when, &error) != CR_OK ||
                 cr_rule_new("vip", 3, when, &named, &error) != CR_OK ||
                 cr_rule_with_payload(named, "discount", 8, limit, &paid, &error) != CR_OK ||
                 cr_rule_with_priority(paid, 10, &vip, &error) != CR_OK;
    if (!failed) {
        const cr_rule *rules[1] = {vip};
        failed = cr_ruleset_new(rules, 1, "priority", 8, NULL, 0, NULL, 0, false, &built, &error) != CR_OK ||
                 cr_ruleset_write(built, CR_FORMAT_TEXT, &text, NULL, &error) != CR_OK;
    }
    if (!failed) {
        printf("\nbuilt in code:\n%s\n", text);
    }
    cr_string_free(text);
    cr_ruleset_free(built);
    cr_rule_free(vip);
    cr_rule_free(paid);
    cr_rule_free(named);
    cr_node_free(when);
    cr_node_free(limit);
    cr_node_free(years);
    cr_value_free(five);
    cr_value_free(path);
    return failed ? fail("building", error) : 0;
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

    /* The same input, built natively; the decision keeps a copy of it. */
    cr_value *native = native_input(&error);
    if (native == NULL) {
        cr_ruleset_free(rules);
        return fail("building the input", error);
    }
    status = cr_ruleset_evaluate_value(rules, native, NULL, &decision, &error);
    cr_value_free(native);
    if (status != CR_OK) {
        cr_ruleset_free(rules);
        return fail("evaluating", error);
    }
    printf("\n");
    int shown = show(rules, decision);
    cr_decision_free(decision);
    if (shown != 0) {
        cr_ruleset_free(rules);
        return shown;
    }

    /* Run the rules: evaluate, then call the selected rules' actions. */
    int calls = 0;
    cr_actions *actions = cr_actions_new();
    native = native_input(&error);
    if (native == NULL ||
        cr_actions_add(actions, "notify_sales", 12, notify_sales, &calls, &error) != CR_OK ||
        cr_ruleset_run(rules, actions, native, NULL, &decision, &error) != CR_OK) {
        cr_value_free(native);
        cr_actions_free(actions);
        cr_ruleset_free(rules);
        return fail("running", error);
    }
    printf("\n%d action called\n", calls);
    cr_decision_free(decision);
    cr_value_free(native);
    cr_actions_free(actions);

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
    return expected ? built_in_code() : 1;
}
