// The typed API under `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`
// (tsconfig.unchecked.json): with typed keys (`Fields<T>`) or `field(...)`,
// nothing is possibly undefined.

import { type Expr, F, type Field, type FieldRef, type Fields, Rule, RuleSet, decimal, field } from 'cleanrules'

interface Order {
  customer: { years: number; newsletter?: boolean; address: { city: string } | null }
  order: { total: number }
  items: Array<{ price: number }>
}

const O = F as Fields<Order>
const years: Field = O.customer.years
const city: Field = O.customer.address.city
const optional: Field = O.customer.newsletter
const price: Field = O.items.at(0).price
const vipWhen: Expr = O.customer.years.gt(5).and(O.order.total.gt(1000))

// The README example, with typed keys.
const rules = new RuleSet(
  [
    new Rule('vip', vipWhen, { discount: decimal('0.15') }, {
      priority: 10,
      do: ({ rule, output }) => void `${rule.name}: ${String(output['discount'])}`,
    }),
    new Rule('loyal', O.customer.years.gt(2), { discount: decimal('0.05') }, { priority: 5 }),
  ],
  { hitPolicy: 'priority' },
)
const decision = rules.evaluate({ customer: { years: 7 }, order: { total: 900 } })
const discount = decision.output?.['discount']
const causes = decision.whyNot('vip').causes
const gap = causes[0]?.counterfactual?.gap
const text: string = String(decision.explain())

// Without types, `field(...)` never gives undefined.
const total: FieldRef = field('order', 'total')

// @ts-expect-error: typed keys are checked.
void O.customer.age
// @ts-expect-error: an index is refused (use .at(i)).
void O.items[0].price
// `then` is never a field: `await` gives the field back.
const then: undefined = F.customer?.then

void [years, city, optional, price, discount, gap, text, total, then]
