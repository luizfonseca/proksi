// Rules (ADR 0007) as the host pages show them: conditions as a tree, actions with their settings

export type Rule = { name?: string; then?: Record<string, unknown>; [key: string]: unknown };

/** A test (`method is one of GET, HEAD`), or `all`, `any` or `none` of the tests below it */
export type Condition = { test: string } | { group: 'all' | 'any' | 'none'; children: Condition[] };

const GROUPS = { and: 'all', or: 'any', not: 'none' } as const;
const LABELLED = ['header', 'cookie', 'query'];
const OPERATORS: Record<string, string> = {
  eq: 'is',
  neq: 'is not',
  in: 'is one of',
  not_in: 'is none of',
  prefix: 'starts with',
  suffix: 'ends with',
  contains: 'contains',
  wildcard: 'matches',
  matches: 'matches the expression',
};

function text(value: unknown): string {
  if (Array.isArray(value)) return value.map(text).join(', ');
  if (value && typeof value === 'object') {
    return Object.entries(value)
      .map(([key, inner]) => `${key} ${text(inner)}`)
      .join(', ');
  }
  return String(value);
}

function tests(field: string, operators: unknown): Condition[] {
  if (Array.isArray(operators)) return operators.flatMap((each) => tests(field, each));
  if (!operators || typeof operators !== 'object') return [{ test: `${field} ${text(operators)}` }];
  return Object.entries(operators).map(([operator, value]) => {
    if (operator === 'exists') return { test: `${field} ${value ? 'is there' : 'is missing'}` };
    return { test: `${field} ${OPERATORS[operator] ?? operator} ${text(value)}` };
  });
}

/** The tests of a block's body: each key is a field or a nested `and`, `or`, `not` */
function body(node: unknown): Condition[] {
  if (Array.isArray(node)) return node.flatMap(body);
  if (!node || typeof node !== 'object') return [];
  return Object.entries(node).flatMap(([key, value]): Condition[] => {
    if (key in GROUPS) {
      // Several blocks of a kind are a list: each is a group of its own
      const blocks = Array.isArray(value) ? value : [value];
      return blocks.map((block) => ({ group: GROUPS[key as keyof typeof GROUPS], children: body(block) }));
    }
    if (LABELLED.includes(key) && value && typeof value === 'object') {
      return Object.entries(value).flatMap(([label, operators]) => tests(`${key} "${label}"`, operators));
    }
    return tests(key, value);
  });
}

/** When the rule holds: its `and`, `or` or `not` */
export function condition(rule: Rule): Condition[] {
  const { name: _name, then: _then, ...when } = rule;
  return body(when);
}

/** What the rule does: each block, with its settings */
export function actions(rule: Rule): { block: string; settings: string }[] {
  return Object.entries(rule.then ?? {}).map(([block, settings]) => ({
    block,
    settings: settings === '(hidden)' ? 'settings hidden' : text(settings),
  }));
}
