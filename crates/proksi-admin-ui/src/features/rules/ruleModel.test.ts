import { describe, expect, it } from 'vitest';
import { emptyRule, fromRule, type Group, toRule } from './ruleModel';

describe('the rule builder model', () => {
  it('reads and writes rules as Proksi takes them', () => {
    const rule = {
      name: 'partners',
      and: {
        method: { in: ['POST', 'PUT'] },
        header: { 'x-partner': { exists: true } },
        or: [{ path: { prefix: '/api' } }, { not: { client: { in: ['10.0.0.0/8'] } } }],
      },
      then: { rate_limit: { requests: 10, per: '1m' } },
    };

    const model = fromRule(rule);

    expect(model.name).toBe('partners');
    expect(model.when.group).toBe('and');
    expect(model.when.children).toHaveLength(4);
    expect(model.when.children[1]).toEqual({ kind: 'test', field: 'header', name: 'x-partner', operator: 'exists', value: true });
    expect(toRule(model)).toEqual(rule);
  });

  it('writes repeated tests as lists', () => {
    const when: Group = {
      kind: 'group',
      group: 'and',
      children: [
        { kind: 'test', field: 'path', name: '', operator: 'prefix', value: '/a' },
        { kind: 'test', field: 'path', name: '', operator: 'suffix', value: '.json' },
        { kind: 'test', field: 'header', name: 'a', operator: 'eq', value: '1' },
        { kind: 'test', field: 'header', name: 'b', operator: 'eq', value: '2' },
        { kind: 'test', field: 'header', name: 'a', operator: 'neq', value: '3' },
      ],
    };
    expect(toRule({ name: 'r', when, then: {} })).toEqual({
      name: 'r',
      and: {
        path: [{ prefix: '/a' }, { suffix: '.json' }],
        header: { a: [{ eq: '1' }, { neq: '3' }], b: { eq: '2' } },
      },
      then: {},
    });
    expect(fromRule(toRule({ name: 'r', when, then: {} })).when.children).toHaveLength(5);
  });

  it('starts new rules with one test', () => {
    const rule = emptyRule();
    expect(rule.when.group).toBe('and');
    expect(rule.when.children).toHaveLength(1);
  });

  it("doesn't read what it can't edit", () => {
    expect(() => fromRule({ name: 'a', method: { eq: 'GET' }, then: {} })).toThrow();
    expect(() => fromRule({ name: 'a', and: {}, or: {}, then: {} })).toThrow();
  });
});
