// What rules and blocks accept (`GET /api/v1/schema`), and reading its JSON Schemas

import { useEffect, useState } from 'preact/hooks';
import { get } from '../../api';

export type JsonSchema = {
  type?: string | string[];
  description?: string;
  properties?: Record<string, JsonSchema>;
  required?: string[];
  items?: JsonSchema;
  enum?: string[];
  $ref?: string;
  $defs?: Record<string, JsonSchema>;
  anyOf?: JsonSchema[];
  additionalProperties?: boolean | JsonSchema;
  minimum?: number;
  maximum?: number;
  default?: unknown;
};

export type FieldInfo = { name: string; named: boolean; description: string; operators: string[] };
export type OperatorInfo = { name: string; value: 'text' | 'list' | 'bool'; description: string };

export type Schema = {
  rules: {
    fields: FieldInfo[];
    operators: OperatorInfo[];
    groups: { name: string; description: string }[];
    actions: Record<string, JsonSchema>;
  };
  blocks: Record<string, JsonSchema>;
};

let cached: Promise<Schema> | null = null;

/** The schema, fetched once a page load: it's that of the Proksi serving the page */
export function useSchema(): Schema | null {
  const [schema, setSchema] = useState<Schema | null>(null);
  useEffect(() => {
    cached ??= get<Schema>('/schema');
    cached.then(setSchema).catch(() => {
      cached = null;
    });
  }, []);
  return schema;
}

/** `schema` without `$ref` and `null`: what a value of it is */
export function resolve(schema: JsonSchema, root: JsonSchema): JsonSchema {
  if (schema.$ref) {
    const name = schema.$ref.split('/').pop() ?? '';
    const target = root.$defs?.[name] ?? {};
    return { ...resolve(target, root), description: schema.description ?? target.description };
  }
  if (schema.anyOf) {
    const other = schema.anyOf.find((option) => option.type !== 'null');
    return other ? { ...resolve(other, root), description: schema.description ?? other.description } : schema;
  }
  if (Array.isArray(schema.type)) {
    return { ...schema, type: schema.type.find((type) => type !== 'null') };
  }
  return schema;
}

/** The kind of input a setting takes */
export type Input = 'text' | 'integer' | 'boolean' | 'choice' | 'list' | 'choices' | 'map' | 'object' | 'objects' | 'unknown';

export function input(schema: JsonSchema, root: JsonSchema): Input {
  const resolved = resolve(schema, root);
  if (resolved.enum) return 'choice';
  switch (resolved.type) {
    case 'string':
      return 'text';
    case 'integer':
    case 'number':
      return 'integer';
    case 'boolean':
      return 'boolean';
    case 'array': {
      const items = resolve(resolved.items ?? {}, root);
      if (items.enum) return 'choices';
      if (items.type === 'object') return 'objects';
      return 'list';
    }
    case 'object':
      return typeof resolved.additionalProperties === 'object' ? 'map' : 'object';
    default:
      return 'unknown';
  }
}
