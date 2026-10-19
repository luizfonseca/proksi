import { describe, expect, it } from 'vitest';
import { add, addAll, type Served, SHOWN } from './requests';

const served = (path: string): Served => ({
  time: 0,
  method: 'GET',
  host: 'a.test',
  path,
  status: 200,
  duration_ms: 1,
  client_ip: '127.0.0.1',
  location: '/',
  upstream: null,
});

describe('the latest requests', () => {
  it('keeps the newest first, up to 100', () => {
    let requests: Served[] = [];
    for (let index = 0; index < SHOWN + 5; index++) requests = add(requests, served(`/${index}`));
    expect(requests).toHaveLength(SHOWN);
    expect(requests[0].path).toBe(`/${SHOWN + 4}`);
  });

  it('adds what arrived in a second at once, newest first', () => {
    const requests = addAll([served('/old')], [served('/1'), served('/2')]);
    expect(requests.map((one) => one.path)).toEqual(['/2', '/1', '/old']);
    const many = Array.from({ length: SHOWN + 20 }, (_, index) => served(`/${index}`));
    expect(addAll([], many)).toHaveLength(SHOWN);
    expect(addAll([], many)[0].path).toBe(`/${SHOWN + 19}`);
  });
});
