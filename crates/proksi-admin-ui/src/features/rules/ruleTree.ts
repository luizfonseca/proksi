// Rules (ADR 0007) as the host pages show them: conditions as a tree, actions with their settings

export type Rule = { name?: string; then?: Record<string, unknown>; [key: string]: unknown };

/**
 * What a test checks: its field (`header`), the name of what's checked in it (`x-api-key`), how
 * (`is one of`), and the value it's checked against: missing for `is there`, or `hidden` (a secret)
 */
export type Test = { field: string; name?: string; operator: string; value?: string; hidden?: boolean };

/** A test, or `all`, `any` or `none` of the tests below it */
export type Condition = { test: Test } | { group: 'all' | 'any' | 'none'; children: Condition[] };

/** How the API writes a value it doesn't show */
const HIDDEN = '(hidden)';

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

function tests(field: string, name: string | undefined, operators: unknown): Condition[] {
  if (Array.isArray(operators)) return operators.flatMap((each) => tests(field, name, each));
  const named = name === undefined ? { field } : { field, name };
  if (!operators || typeof operators !== 'object') return [{ test: { ...named, operator: 'is', ...value(operators) } }];
  return Object.entries(operators).map(([operator, checked]) => {
    if (operator === 'exists') return { test: { ...named, operator: checked ? 'is there' : 'is missing' } };
    return { test: { ...named, operator: OPERATORS[operator] ?? operator, ...value(checked) } };
  });
}

function value(checked: unknown): Pick<Test, 'value' | 'hidden'> {
  return checked === HIDDEN ? { hidden: true } : { value: text(checked) };
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
      return Object.entries(value).flatMap(([label, operators]) => tests(key, label, operators));
    }
    return tests(key, undefined, value);
  });
}

/** When the rule holds: its `and`, `or` or `not` */
export function condition(rule: Rule): Condition[] {
  const { name: _name, then: _then, ...when } = rule;
  return body(when);
}

/** What the rule does: each block, with its settings by name (or `hidden`, when they're secret) */
export function actions(rule: Rule): { block: string; settings: [string, string][] | 'hidden' }[] {
  return Object.entries(rule.then ?? {}).map(([block, settings]) => ({
    block,
    settings:
      settings === HIDDEN
        ? 'hidden'
        : settings && typeof settings === 'object' && !Array.isArray(settings)
          ? Object.entries(settings).map(([name, inner]): [string, string] => [name, text(inner)])
          : [['', text(settings)]],
  }));
}
