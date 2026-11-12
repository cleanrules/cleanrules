"""The tools for AI agents (`spec/agents.md`) on the rules of `discounts.rules`.

    python docs/examples/agent_tools.py

An agent calls the tools by name with JSON arguments, and gets JSON back: the
same answer from the core in every language and from the MCP server. Tools
never run actions (the rule `vip` names `notify_sales`, which stays data), and
a mistake comes back as a tool error with a pointer and a hint, so the model
can correct its call.
"""

import json
from pathlib import Path

import cleanrules
from cleanrules import Catalog

RULES = cleanrules.load(Path(__file__).with_name("discounts.rules"), format="text")
CATALOG = Catalog({"discounts": RULES})


def show(tool: str, arguments: dict[str, object]) -> None:
    """Call a tool and print what it gave."""
    result = CATALOG.call(tool, arguments)
    print(f"--- {tool} {json.dumps(arguments)}{' (tool error)' if result.is_error else ''}")
    print(json.dumps(result.content, indent=2))


# What the agent may use, and what each rule set needs and decides.
show("list_rulesets", {})
schema = CATALOG.call("summary", {"ruleset": "discounts"}).content["input_schema"]
print("--- input schema of discounts")
print(json.dumps(schema, indent=2))

# A decision, then the same with its explanation cut to fit a model's context.
order = {"customer": {"years": 7}, "order": {"total": 1500}}
show("evaluate", {"ruleset": "discounts", "input": order})
show("explain", {"ruleset": "discounts", "input": order, "budget": 400})

# Mistakes the model can correct: a wrong name, and a rule it wrote.
show("evaluate", {"ruleset": "discount", "input": order})
proposed = "RULE gold IF customer.years >\nTHEN discount = 0.2"
show("validate", {"document": proposed, "format": "text"})
