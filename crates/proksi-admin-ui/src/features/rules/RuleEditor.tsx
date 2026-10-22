import { useState } from 'preact/hooks';
import { emptyRule, emptyTest, fromRule, type Group, type GroupName, type Node, type RuleModel, type Test, type TestValue, toRule } from './ruleModel';
import { type Schema, useSchema } from '../blocks/schema';
import { SchemaForm } from '../blocks/SchemaForm';
import { Alert } from '../../components/ui/Alert';
import { Chip } from '../../components/ui/Badge';
import { Button } from '../../components/ui/Button';
import { Checkbox, Input, ListInput, Select, TextArea } from '../../components/ui/Form';
import { Hint, Label, None } from '../../components/ui/Page';
import { Dropdown } from '../../components/ui/Menu';
import { Grid, Stack } from '../../components/ui/Stack';
import { asList } from '../../lib/lists';

type Rules = Schema['rules'];

const GROUP_LABELS: Record<GroupName, string> = { and: 'All of', or: 'Any of', not: 'None of' };

/** A value of the kind `operator` takes, keeping what it can of `value` */
function convert(value: TestValue, kind: string | undefined): TestValue {
  const words = Array.isArray(value) ? value : typeof value === 'string' && value ? [value] : [];
  if (kind === 'list') return words;
  if (kind === 'bool') return typeof value === 'boolean' ? value : true;
  return words[0] ?? '';
}

function TestRow({ test, rules, onChange, onRemove }: { test: Test; rules: Rules; onChange: (test: Test) => void; onRemove: () => void }) {
  const field = rules.fields.find((candidate) => candidate.name === test.field);
  const operators = rules.operators.filter((operator) => field?.operators.includes(operator.name));
  const kind = rules.operators.find((operator) => operator.name === test.operator)?.value;
  // `exists` reads as "is there" or "is missing": its value is in the operator's choice
  const setOperator = (choice: string) => {
    const [name, missing] = choice.split(':');
    const next = rules.operators.find((operator) => operator.name === name);
    const value = next?.value === 'bool' ? missing !== 'missing' : convert(test.value, next?.value);
    onChange({ ...test, operator: name, value });
  };
  const chosen = kind === 'bool' && test.value === false ? `${test.operator}:missing` : test.operator;
  return (
    <div class="flex flex-nowrap items-center gap-1.5 max-md:flex-wrap">
      <Select
        class="w-[92px] flex-none"
        aria-label="Field"
        title={field?.description}
        value={test.field}
        onChange={(e) => {
          const next = rules.fields.find((candidate) => candidate.name === e.currentTarget.value);
          const operator = next?.operators.includes(test.operator) ? test.operator : (next?.operators[0] ?? 'eq');
          const value = convert(test.value, rules.operators.find((one) => one.name === operator)?.value);
          onChange({ ...test, field: e.currentTarget.value, name: next?.named ? test.name : '', operator, value });
        }}
      >
        {rules.fields.map((option) => (
          <option value={option.name} title={option.description}>
            {option.name}
          </option>
        ))}
      </Select>
      {field?.named && <Input mono class="w-[110px] flex-none" aria-label={`${test.field} name`} placeholder={test.field === 'header' ? 'x-api-key' : 'name'} value={test.name} onInput={(e) => onChange({ ...test, name: e.currentTarget.value })} />}
      <Select class="w-32 flex-none" aria-label="Operator" value={chosen} onChange={(e) => setOperator(e.currentTarget.value)}>
        {operators.flatMap((operator) =>
          operator.value === 'bool'
            ? [<option value={operator.name}>{operator.description}</option>, <option value={`${operator.name}:missing`}>is missing</option>]
            : [<option value={operator.name}>{operator.description}</option>],
        )}
      </Select>
      <div class="min-w-0 flex-1 max-md:min-w-[140px]">
        {kind === 'list' ? (
          <ListInput value={asList(test.value)} onChange={(value) => onChange({ ...test, value })} />
        ) : kind === 'bool' ? (
          // Nothing more to say: "is there", or "is missing"
          <span />
        ) : (
          <Input mono aria-label="Value" value={test.value as string} onInput={(e) => onChange({ ...test, value: e.currentTarget.value })} />
        )}
      </div>
      <Button variant="link-danger" class="w-5 flex-none text-center" aria-label="Remove condition" onClick={onRemove}>
        ✕
      </Button>
    </div>
  );
}

function GroupEditor({ group, rules, onChange, onRemove }: { group: Group; rules: Rules; onChange: (group: Group) => void; onRemove?: () => void }) {
  const set = (children: Node[]) => onChange({ ...group, children });
  const replace = (index: number, node: Node) => set(group.children.map((child, at) => (at === index ? node : child)));
  const remove = (index: number) => set(group.children.filter((_, at) => at !== index));
  return (
    <div class={`grid gap-1.5 group-${group.group} ${onRemove ? 'rounded-field bg-base-200 p-2' : ''}`}>
      <Stack gap={2.5}>
        <Select
          class="border-transparent bg-base-200 font-semibold hover:border-base-300"
          aria-label="Combine" value={group.group} onChange={(e) => onChange({ ...group, group: e.currentTarget.value as GroupName })}>
          {(Object.keys(GROUP_LABELS) as GroupName[]).map((name) => (
            <option value={name}>{GROUP_LABELS[name]}</option>
          ))}
        </Select>
        <span class="text-sm text-base-content/45">{group.group === 'not' ? 'none of these hold' : group.group === 'or' ? 'at least one holds' : 'every one holds'}</span>
        {onRemove && (
          <Button variant="link-danger" class="ml-auto" onClick={onRemove}>
            Remove group
          </Button>
        )}
      </Stack>
      <div class="nested ml-0 grid gap-1.5 pl-3">
        {group.children.length === 0 && <None>Add a condition</None>}
        {group.children.map((child, index) =>
          child.kind === 'test' ? (
            <TestRow test={child} rules={rules} onChange={(test) => replace(index, test)} onRemove={() => remove(index)} />
          ) : (
            <GroupEditor group={child} rules={rules} onChange={(inner) => replace(index, inner)} onRemove={() => remove(index)} />
          ),
        )}
        <div class="flex gap-3.5">
          <Button variant="link" onClick={() => set([...group.children, emptyTest()])}>
            + Condition
          </Button>
          <Button variant="link" onClick={() => set([...group.children, { kind: 'group', group: group.group === 'or' ? 'and' : 'or', children: [emptyTest()] }])}>
            + Group
          </Button>
        </div>
      </div>
    </div>
  );
}

/** "+ Add an action", opening the list of actions with what each does */
function AddAction({ names, rules, onAdd }: { names: string[]; rules: Rules; onAdd: (name: string) => void }) {
  const describe = (name: string) => rules.actions[name]?.description?.replace(/`[^`]*`:\s*/, '').replace(/`/g, '');
  return <Dropdown label="+ Add an action" items={names.map((name) => ({ title: name, detail: describe(name), onSelect: () => onAdd(name) }))} />;
}

function Actions({ then, rules, onChange }: { then: Record<string, unknown>; rules: Rules; onChange: (then: Record<string, unknown>) => void }) {
  const available = Object.keys(rules.actions).filter((name) => !(name in then));
  return (
    <Grid gap={2.5}>
      {Object.entries(then).map(([name, settings]) => {
        const schema = rules.actions[name];
        return (
          <Grid gap={2} class="rounded-field border border-base-300 p-2.5">
            <Stack>
              <Chip>{name}</Chip>
              <Hint>{schema?.description?.replace(/`[^`]*`:\s*/, '').replace(/`/g, '')}</Hint>
              <Button
                variant="link-danger"
                class="ml-auto"
                onClick={() => {
                  const { [name]: _, ...rest } = then;
                  onChange(rest);
                }}
              >
                Remove
              </Button>
            </Stack>
            {schema?.type === 'boolean' ? (
              // An action turning something on or off, such as `mirror`
              <Checkbox label={settings === true ? 'yes' : 'no'} checked={settings === true} onChange={(checked) => onChange({ ...then, [name]: checked })} />
            ) : schema?.type === 'string' ? (
              // An action taking a name, such as `traffic_split`
              <Input mono value={typeof settings === 'string' ? settings : ''} onInput={(e) => onChange({ ...then, [name]: e.currentTarget.value })} />
            ) : schema ? (
              <SchemaForm schema={schema} value={(settings as Record<string, unknown>) ?? {}} onChange={(value) => onChange({ ...then, [name]: value })} />
            ) : (
              <None>Proksi doesn't know this action</None>
            )}
          </Grid>
        );
      })}
      {available.length > 0 && (
        <AddAction names={available} rules={rules} onAdd={(name) => {
            const type = rules.actions[name]?.type;
            onChange({ ...then, [name]: type === 'string' ? '' : type === 'boolean' ? true : {} });
          }} />
      )}
    </Grid>
  );
}

/** Moves a rule up or down, greyed out at the ends */
function Move({ label, disabled, onClick, children }: { label: string; disabled: boolean; onClick: () => void; children: string }) {
  return (
    <Button variant="link" class="disabled:cursor-default disabled:text-base-content/30 disabled:no-underline" disabled={disabled} aria-label={label} onClick={onClick}>
      {children}
    </Button>
  );
}

/** One rule: its name, when it holds, and what it does; as JSON when the builder can't read it */
function RuleCard({ rule, index, count, rules, onChange, onMove, onRemove }: { rule: unknown; index: number; count: number; rules: Rules; onChange: (rule: unknown) => void; onMove: (by: number) => void; onRemove: () => void }) {
  const [asJson, setAsJson] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  let model: RuleModel | null = null;
  let unreadable: string | null = null;
  try {
    model = fromRule(rule);
  } catch (err) {
    unreadable = (err as Error).message;
  }
  const json = asJson || !model;
  return (
    <li class="rounded-box border border-base-content/15 bg-base-100">
      <div class="flex items-center gap-2.5 border-b border-base-300 px-3 py-2">
        <span class="grid size-6 flex-none place-items-center rounded-md bg-base-200 text-xs font-semibold text-base-content/60">{index + 1}</span>
        {model ? (
          <input
            class="w-full min-w-0 border-b border-transparent bg-transparent px-0.5 py-1 font-semibold outline-none focus:border-primary" placeholder="Name this rule" value={model.name} onInput={(e) => onChange(toRule({ ...model!, name: e.currentTarget.value }))} />
        ) : (
          <strong>{(rule as { name?: string })?.name ?? 'Rule'}</strong>
        )}
        <div class="ml-auto flex flex-none gap-3">
          <Move label="Move up" disabled={index === 0} onClick={() => onMove(-1)}>
            ↑
          </Move>
          <Move label="Move down" disabled={index === count - 1} onClick={() => onMove(1)}>
            ↓
          </Move>
          {model && (
            <Button variant="link" onClick={() => setAsJson(!asJson)}>
              {asJson ? 'Builder' : 'JSON'}
            </Button>
          )}
          <Button variant="link-danger" onClick={onRemove}>
            Remove
          </Button>
        </div>
      </div>
      {json ? (
        <Grid gap={2} class="p-3">
          {unreadable && <Hint>The builder can't edit this rule ({unreadable}): edit it as JSON.</Hint>}
          <TextArea
            mono
            class="leading-normal"
            spellcheck={false}
            rows={10}
            value={draft ?? JSON.stringify(rule, null, 2)}
            onInput={(e) => setDraft(e.currentTarget.value)}
            onBlur={() => {
              if (draft === null) return;
              try {
                onChange(JSON.parse(draft));
                setDraft(null);
                setError(null);
              } catch (err) {
                setError((err as Error).message);
              }
            }}
          />
          {error && <Alert>Not valid JSON: {error}</Alert>}
        </Grid>
      ) : (
        model && (
          <div class="grid grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)] max-lg:grid-cols-1">
            <section class="min-w-0 p-3">
              <Label>When</Label>
              <GroupEditor group={model.when} rules={rules} onChange={(when) => onChange(toRule({ ...model!, when }))} />
            </section>
            <section class="min-w-0 border-l border-base-300 p-3 max-lg:border-t max-lg:border-l-0">
              <Label>Then</Label>
              <Actions then={model.then} rules={rules} onChange={(then) => onChange(toRule({ ...model!, then }))} />
            </section>
          </div>
        )
      )}
    </li>
  );
}

/** A scope's rules, in the order they run */
export function RulesEditor({ rules, onChange }: { rules: unknown[]; onChange: (rules: unknown[]) => void }) {
  const schema = useSchema();
  if (!schema) return <None>Loading what rules accept…</None>;
  const move = (index: number, by: number) => {
    const next = rules.slice();
    const [rule] = next.splice(index, 1);
    next.splice(index + by, 0, rule);
    onChange(next);
  };
  return (
    <div class="grid">
      <ol class="grid gap-2.5">
        {rules.map((rule, index) => (
          <RuleCard
            rule={rule}
            index={index}
            count={rules.length}
            rules={schema.rules}
            onChange={(next) => onChange(rules.map((one, at) => (at === index ? next : one)))}
            onMove={(by) => move(index, by)}
            onRemove={() => onChange(rules.filter((_, at) => at !== index))}
          />
        ))}
      </ol>
      <Button variant="link" class="mt-2" onClick={() => onChange([...rules, toRule(emptyRule())])}>
        + Add rule
      </Button>
    </div>
  );
}
