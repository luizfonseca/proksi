import { Chip } from '../../components/ui/Badge';
import { Hint, Label, Mono, None } from '../../components/ui/Page';
import { Grid } from '../../components/ui/Stack';
import { actions, type Condition, condition, type Rule } from './ruleTree';

const GROUP_LABEL = { all: 'all of', any: 'any of', none: 'none of' };

function Tree({ conditions, nested }: { conditions: Condition[]; nested?: boolean }) {
  return (
    <ul class={`grid gap-1 text-xs ${nested ? 'nested mt-1' : ''}`}>
      {conditions.map((item) =>
        'test' in item ? (
          <li><Mono>{item.test}</Mono></li>
        ) : (
          <li>
            <span class="text-xs text-base-content/60">{GROUP_LABEL[item.group]}</span>
            <Tree conditions={item.children} nested />
          </li>
        ),
      )}
    </ul>
  );
}

/** A scope's rules, in the order they run */
export function Rules({ rules }: { rules: Rule[] }) {
  if (rules.length === 0) return <None>None</None>;
  return (
    <ol class="grid gap-2">
      {rules.map((rule, index) => (
        <li class="rounded-lg border border-base-300 px-3 py-2.5">
          <div class="mb-2 text-sm font-semibold">
            <span class="font-medium text-base-content/45">{index + 1}. </span>
            {rule.name ?? 'unnamed'}
          </div>
          <Grid columns={2} gap={4}>
            <div>
              <Label>When</Label>
              <Tree conditions={condition(rule)} />
            </div>
            <div>
              <Label>Then</Label>
              <ul class="grid gap-1 text-xs">
                {actions(rule).map(({ block, settings }) => (
                  <li>
                    <Chip>{block}</Chip> <Hint class="font-mono text-base-content/60">{settings}</Hint>
                  </li>
                ))}
              </ul>
            </div>
          </Grid>
        </li>
      ))}
    </ol>
  );
}
