import { Chip } from '../../components/ui/Badge';
import { Mono, None } from '../../components/ui/Page';
import { actions, type Condition, condition, type Rule, type Test } from './ruleTree';

const GROUP_LABEL = { all: 'all of', any: 'any of', none: 'none of' };

/** A test as a phrase: `header x-api-key is not ••••` */
function Phrase({ test }: { test: Test }) {
  return (
    <span class="inline-flex flex-wrap items-baseline gap-x-1.5">
      <span class="text-base-content/60">{test.field}</span>
      {test.name && <Mono class="font-semibold">{test.name}</Mono>}
      <span class="text-base-content/60">{test.operator}</span>
      {test.hidden ? (
        <span class="text-xs text-base-content/45 italic">hidden value</span>
      ) : (
        test.value !== undefined && <Mono class="rounded bg-base-200 px-1.5 py-0.5">{test.value}</Mono>
      )}
    </span>
  );
}

function Conditions({ conditions, nested }: { conditions: Condition[]; nested?: boolean }) {
  return (
    <ul class={`grid gap-1.5 ${nested ? 'nested mt-1.5' : ''}`}>
      {conditions.map((item) =>
        'test' in item ? (
          <li>
            <Phrase test={item.test} />
          </li>
        ) : (
          <li class={`group-${item.group === 'any' ? 'or' : item.group === 'none' ? 'not' : 'and'}`}>
            <span class="text-xs font-medium text-base-content/60">{GROUP_LABEL[item.group]}</span>
            <Conditions conditions={item.children} nested />
          </li>
        ),
      )}
    </ul>
  );
}

/** The conditions to show: a single "all of" around them says nothing */
function shown(rule: Rule): Condition[] {
  const conditions = condition(rule);
  const only = conditions[0];
  return conditions.length === 1 && only && 'group' in only && only.group === 'all' ? only.children : conditions;
}

/** A small word before what follows it on its line */
function Lead({ children }: { children: string }) {
  return <span class="w-10 shrink-0 pt-0.5 text-xs font-medium tracking-wide text-base-content/45 uppercase">{children}</span>;
}

/** A scope's rules, in the order they run: each as "if … then …" */
export function Rules({ rules }: { rules: Rule[] }) {
  if (rules.length === 0) return <None>None</None>;
  return (
    <ol class="grid divide-y divide-base-300">
      {rules.map((rule, index) => {
        const conditions = shown(rule);
        return (
          <li class="flex gap-3 py-3 text-sm first:pt-0 last:pb-0">
            <span class="grid size-6 shrink-0 place-items-center rounded-full bg-base-200 text-xs font-semibold text-base-content/60">
              {index + 1}
            </span>
            <div class="grid min-w-0 gap-2">
              <div class="font-semibold">{rule.name ?? 'Unnamed rule'}</div>
              <div class="flex gap-2">
                <Lead>If</Lead>
                {conditions.length === 0 ? <None>always</None> : <Conditions conditions={conditions} />}
              </div>
              <div class="flex gap-2">
                <Lead>Then</Lead>
                <ul class="grid gap-1.5">
                  {actions(rule).map(({ block, settings }) => (
                    <li class="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                      <Chip>{block}</Chip>
                      {settings === 'hidden' ? (
                        <span class="text-xs text-base-content/45 italic">settings hidden</span>
                      ) : (
                        settings.map(([name, value]) => (
                          <span class="inline-flex items-baseline gap-1">
                            {name && <span class="text-base-content/60">{name}</span>}
                            <Mono>{value}</Mono>
                          </span>
                        ))
                      )}
                    </li>
                  ))}
                </ul>
              </div>
            </div>
          </li>
        );
      })}
    </ol>
  );
}
