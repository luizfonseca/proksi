import { describe, expect, it } from 'vitest';
import { input, type JsonSchema, resolve } from './schema';

const root: JsonSchema = {
  $defs: {
    Provider: { type: 'string', enum: ['github', 'workos'], description: 'A provider' },
    Changes: { type: 'object', properties: { add: { type: 'object', additionalProperties: { type: 'string' } } } },
    Validation: { type: 'object', properties: { value: { type: 'array', items: { type: 'string' } } } },
  },
  properties: {},
};

describe('the schema', () => {
  it('reads what each setting takes', () => {
    expect(input({ type: 'string' }, root)).toBe('text');
    expect(input({ type: ['integer', 'null'] }, root)).toBe('integer');
    expect(input({ type: ['boolean', 'null'] }, root)).toBe('boolean');
    expect(input({ $ref: '#/$defs/Provider' }, root)).toBe('choice');
    expect(input({ type: ['array', 'null'], items: { type: 'string' } }, root)).toBe('list');
    expect(input({ type: 'array', items: { $ref: '#/$defs/Provider' } }, root)).toBe('choices');
    expect(input({ type: 'array', items: { $ref: '#/$defs/Validation' } }, root)).toBe('objects');
    expect(input({ type: 'object', additionalProperties: { type: 'string' } }, root)).toBe('map');
    expect(input({ anyOf: [{ $ref: '#/$defs/Changes' }, { type: 'null' }] }, root)).toBe('object');
  });

  it('keeps the description of a reference', () => {
    expect(resolve({ $ref: '#/$defs/Provider', description: 'Which one' }, root).description).toBe('Which one');
    expect(resolve({ $ref: '#/$defs/Provider' }, root).description).toBe('A provider');
  });
});
