import { useState } from 'preact/hooks';
import { emptyRule, emptyTest, fromRule, type Group, type GroupName, type Node, type RuleModel, type Test, type TestValue, toRule } from '../ruleModel';
import { type Schema, useSchema } from '../schema';
import { ListInput, SchemaForm } from './SchemaForm';

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
    <div class="test-row">
      <select
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
      </select>
      {field?.named && <input class="mono name" aria-label={`${test.field} name`} placeholder={test.field === 'header' ? 'x-api-key' : 'name'} value={test.name} onInput={(e) => onChange({ ...test, name: e.currentTarget.value })} />}
      <select aria-label="Operator" value={chosen} onChange={(e) => setOperator(e.currentTarget.value)}>
        {operators.flatMap((operator) =>
          operator.value === 'bool'
            ? [<option value={operator.name}>{operator.description}</option>, <option value={`${operator.name}:missing`}>is missing</option>]
            : [<option value={operator.name}>{operator.description}</option>],
        )}
      </select>
      <div class="test-value">
        {kind === 'list' ? (
          <ListInput value={test.value as string[]} onChange={(value) => onChange({ ...test, value })} />
        ) : kind === 'bool' ? (
          // Nothing more to say: "is there", or "is missing"
          <span />
        ) : (
          <input class="mono" aria-label="Value" value={test.value as string} onInput={(e) => onChange({ ...test, value: e.currentTarget.value })} />
        )}
      </div>
      <button type="button" class="link danger" aria-label="Remove condition" onClick={onRemove}>
        ✕
      </button>
    </div>
  );
}

function GroupEditor({ group, rules, onChange, onRemove }: { group: Group; rules: Rules; onChange: (group: Group) => void; onRemove?: () => void }) {
  const set = (children: Node[]) => onChange({ ...group, children });
  const replace = (index: number, node: Node) => set(group.children.map((child, at) => (at === index ? node : child)));
  const remove = (index: number) => set(group.children.filter((_, at) => at !== index));
  return (
    <div class={`group ${group.group}`}>
      <div class="group-head">
        <select aria-label="Combine" value={group.group} onChange={(e) => onChange({ ...group, group: e.currentTarget.value as GroupName })}>
          {(Object.keys(GROUP_LABELS) as GroupName[]).map((name) => (
            <option value={name}>{GROUP_LABELS[name]}</option>
          ))}
        </select>
        <span class="hint">{group.group === 'not' ? 'none of these hold' : group.group === 'or' ? 'at least one holds' : 'every one holds'}</span>
        {onRemove && (
          <button type="button" class="link danger" onClick={onRemove}>
            Remove group
          </button>
        )}
      </div>
      <div class="group-body">
        {group.children.length === 0 && <span class="none">Add a condition</span>}
        {group.children.map((child, index) =>
          child.kind === 'test' ? (
            <TestRow test={child} rules={rules} onChange={(test) => replace(index, test)} onRemove={() => remove(index)} />
          ) : (
            <GroupEditor group={child} rules={rules} onChange={(inner) => replace(index, inner)} onRemove={() => remove(index)} />
          ),
        )}
        <div class="group-actions">
          <button type="button" class="link" onClick={() => set([...group.children, emptyTest()])}>
            + Condition
          </button>
          <button type="button" class="link" onClick={() => set([...group.children, { kind: 'group', group: group.group === 'or' ? 'and' : 'or', children: [emptyTest()] }])}>
            + Group
          </button>
        </div>
      </div>
    </div>
  );
}

/** "+ Add an action", opening the list of actions with what each does */
function AddAction({ names, rules, onAdd }: { names: string[]; rules: Rules; onAdd: (name: string) => void }) {
  const [open, setOpen] = useState(false);
  const describe = (name: string) => rules.actions[name]?.description?.replace(/`[^`]*`:\s*/, '').replace(/`/g, '');
  return (
    <div
      class="action-menu"
      onKeyDown={(e) => e.key === 'Escape' && setOpen(false)}
      onFocusOut={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as globalThis.Node | null)) setOpen(false);
      }}
    >
      <button type="button" class="secondary" aria-expanded={open} onClick={() => setOpen(!open)}>
        + Add an action
      </button>
      {open && (
        <ul role="menu">
          {names.map((name) => (
            <li>
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  onAdd(name);
                  setOpen(false);
                }}
              >
                <span class="mono">{name}</span>
                <small>{describe(name)}</small>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function Actions({ then, rules, onChange }: { then: Record<string, unknown>; rules: Rules; onChange: (then: Record<string, unknown>) => void }) {
  const available = Object.keys(rules.actions).filter((name) => !(name in then));
  return (
    <div class="actions-editor">
      {Object.entries(then).map(([name, settings]) => {
        const schema = rules.actions[name];
        return (
          <div class="action">
            <div class="action-head">
              <span class="chip">{name}</span>
              <span class="hint">{schema?.description?.replace(/`[^`]*`:\s*/, '').replace(/`/g, '')}</span>
              <button
                type="button"
                class="link danger"
                onClick={() => {
                  const { [name]: _, ...rest } = then;
                  onChange(rest);
                }}
              >
                Remove
              </button>
            </div>
            {schema?.type === 'boolean' ? (
              // An action turning something on or off, such as `mirror`
              <label class="check">
                <input type="checkbox" checked={settings === true} onChange={(e) => onChange({ ...then, [name]: e.currentTarget.checked })} />
                {settings === true ? 'yes' : 'no'}
              </label>
            ) : schema?.type === 'string' ? (
              // An action taking a name, such as `traffic_split`
              <input class="mono" value={typeof settings === 'string' ? settings : ''} onInput={(e) => onChange({ ...then, [name]: e.currentTarget.value })} />
            ) : schema ? (
              <SchemaForm schema={schema} value={(settings as Record<string, unknown>) ?? {}} onChange={(value) => onChange({ ...then, [name]: value })} />
            ) : (
              <span class="none">Proksi doesn't know this action</span>
            )}
          </div>
        );
      })}
      {available.length > 0 && (
        <AddAction names={available} rules={rules} onAdd={(name) => {
            const type = rules.actions[name]?.type;
            onChange({ ...then, [name]: type === 'string' ? '' : type === 'boolean' ? true : {} });
          }} />
      )}
    </div>
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
    <li class="rule-card">
      <div class="rule-card-head">
        <span class="number">{index + 1}</span>
        {model ? (
          <input class="rule-name" placeholder="Name this rule" value={model.name} onInput={(e) => onChange(toRule({ ...model!, name: e.currentTarget.value }))} />
        ) : (
          <strong>{(rule as { name?: string })?.name ?? 'Rule'}</strong>
        )}
        <div class="rule-card-tools">
          <button type="button" class="link" disabled={index === 0} aria-label="Move up" onClick={() => onMove(-1)}>
            ↑
          </button>
          <button type="button" class="link" disabled={index === count - 1} aria-label="Move down" onClick={() => onMove(1)}>
            ↓
          </button>
          {model && (
            <button type="button" class="link" onClick={() => setAsJson(!asJson)}>
              {asJson ? 'Builder' : 'JSON'}
            </button>
          )}
          <button type="button" class="link danger" onClick={onRemove}>
            Remove
          </button>
        </div>
      </div>
      {json ? (
        <div class="rule-json">
          {unreadable && <p class="hint">The builder can't edit this rule ({unreadable}): edit it as JSON.</p>}
          <textarea
            class="mono"
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
          {error && <div class="error">Not valid JSON: {error}</div>}
        </div>
      ) : (
        model && (
          <div class="rule-builder">
            <section>
              <h4>When</h4>
              <GroupEditor group={model.when} rules={rules} onChange={(when) => onChange(toRule({ ...model!, when }))} />
            </section>
            <section>
              <h4>Then</h4>
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
  if (!schema) return <span class="none">Loading what rules accept…</span>;
  const move = (index: number, by: number) => {
    const next = rules.slice();
    const [rule] = next.splice(index, 1);
    next.splice(index + by, 0, rule);
    onChange(next);
  };
  return (
    <div class="rules-editor">
      <ol>
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
      <button type="button" class="link" onClick={() => onChange([...rules, toRule(emptyRule())])}>
        + Add rule
      </button>
    </div>
  );
}
