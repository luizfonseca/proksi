import { actions, type Condition, condition, type Rule } from '../rules';

const GROUP_LABEL = { all: 'all of', any: 'any of', none: 'none of' };

function Tree({ conditions }: { conditions: Condition[] }) {
  return (
    <ul class="condition">
      {conditions.map((item) =>
        'test' in item ? (
          <li class="mono">{item.test}</li>
        ) : (
          <li>
            <span class="group">{GROUP_LABEL[item.group]}</span>
            <Tree conditions={item.children} />
          </li>
        ),
      )}
    </ul>
  );
}

/** A scope's rules, in the order they run */
export function Rules({ rules }: { rules: Rule[] }) {
  if (rules.length === 0) return <span class="none">None</span>;
  return (
    <ol class="rules">
      {rules.map((rule) => (
        <li class="rule">
          <div class="rule-name">{rule.name ?? 'unnamed'}</div>
          <div class="rule-body">
            <div>
              <h4>When</h4>
              <Tree conditions={condition(rule)} />
            </div>
            <div>
              <h4>Then</h4>
              <ul class="actions">
                {actions(rule).map(({ block, settings }) => (
                  <li>
                    <span class="chip">{block}</span> <span class="settings">{settings}</span>
                  </li>
                ))}
              </ul>
            </div>
          </div>
        </li>
      ))}
    </ol>
  );
}
