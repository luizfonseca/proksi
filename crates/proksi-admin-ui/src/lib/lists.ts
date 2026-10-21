/** A setting taking one value or a list (`prefix = "/v2"` or `prefix = ["/v2", "/v3"]`), as
 * a list: Proksi accepts both, so either can be stored */
export function asList(value: unknown): string[] {
  if (Array.isArray(value)) return value.map(String);
  if (value === null || value === undefined || value === '') return [];
  return [String(value)];
}
