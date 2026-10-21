// A rule (ADR 0007) as the builder edits it: a tree of `and`, `or` and `not` groups and tests,
// read from and written to the JSON Proksi takes (repeated keys are lists, named fields nest)

export type GroupName = 'and' | 'or' | 'not';
export type TestValue = string | string[] | boolean;

export type Test = {
  kind: 'test';
  field: string;
  /** The header, cookie or query parameter tested (empty for other fields) */
  name: string;
  operator: string;
  value: TestValue;
};
export type Group = { kind: 'group'; group: GroupName; children: Node[] };
export type Node = Test | Group;

export type RuleModel = { name: string; when: Group; then: Record<string, unknown> };

const GROUPS: GroupName[] = ['and', 'or', 'not'];
const NAMED = ['header', 'cookie', 'query'];

type Json = Record<string, unknown>;
const isObject = (value: unknown): value is Json => typeof value === 'object' && value !== null && !Array.isArray(value);
const each = (value: unknown): unknown[] => (Array.isArray(value) ? value : [value]);

export const emptyTest = (): Test => ({ kind: 'test', field: 'path', name: '', operator: 'prefix', value: '/' });

export function emptyRule(): RuleModel {
  return { name: '', when: { kind: 'group', group: 'and', children: [emptyTest()] }, then: { respond: { status: 403 } } };
}

function tests(field: string, name: string, operators: unknown): Test[] {
  return each(operators).flatMap((test) => {
    if (!isObject(test)) throw new Error(`${field}: a test must be { operator: value }`);
    return Object.entries(test).map(([operator, value]) => ({
      kind: 'test' as const,
      field,
      name,
      operator,
      value: value as TestValue,
    }));
  });
}

/** The children of a group's body */
function children(body: unknown): Node[] {
  return each(body).flatMap((block) => {
    if (!isObject(block)) throw new Error('a group must hold conditions');
    return Object.entries(block).flatMap(([key, value]): Node[] => {
      if ((GROUPS as string[]).includes(key)) {
        return each(value).map((inner) => ({ kind: 'group', group: key as GroupName, children: children(inner) }));
      }
      if (NAMED.includes(key)) {
        if (!isObject(value)) throw new Error(`${key} needs a name`);
        return Object.entries(value).flatMap(([name, operators]) => tests(key, name, operators));
      }
      return tests(key, '', value);
    });
  });
}

/** A rule as the builder edits it; throws for what it can't (edit those as JSON) */
export function fromRule(rule: unknown): RuleModel {
  if (!isObject(rule)) throw new Error('a rule is an object');
  const groups = GROUPS.filter((group) => group in rule);
  const others = Object.keys(rule).filter((key) => !['name', 'then', ...GROUPS].includes(key));
  if (groups.length !== 1 || others.length > 0) {
    throw new Error('a rule has one condition: an `and`, `or` or `not` block');
  }
  const group = groups[0];
  return {
    name: typeof rule.name === 'string' ? rule.name : '',
    when: { kind: 'group', group, children: children(rule[group]) },
    then: isObject(rule.then) ? rule.then : {},
  };
}

/** Adds `value` under `key`: a second one makes a list */
function add(body: Json, key: string, value: unknown) {
  if (!(key in body)) body[key] = value;
  else if (Array.isArray(body[key])) (body[key] as unknown[]).push(value);
  else body[key] = [body[key], value];
}

function body(group: Group): Json {
  const json: Json = {};
  for (const child of group.children) {
    if (child.kind === 'group') {
      add(json, child.group, body(child));
    } else if (NAMED.includes(child.field)) {
      const named = (json[child.field] ??= {}) as Json;
      add(named, child.name, { [child.operator]: child.value });
    } else {
      add(json, child.field, { [child.operator]: child.value });
    }
  }
  return json;
}

/** The JSON Proksi takes */
export function toRule(model: RuleModel): Json {
  return { name: model.name, [model.when.group]: body(model.when), then: model.then };
}
