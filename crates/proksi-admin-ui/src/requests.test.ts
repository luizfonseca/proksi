import { describe, expect, it } from 'vitest';
import { add, addAll, KEPT, matches, type Served } from './requests';

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
  it('keeps the newest first, up to 1000', () => {
    let requests: Served[] = [];
    for (let index = 0; index < KEPT + 5; index++) requests = add(requests, served(`/${index}`));
    expect(requests).toHaveLength(KEPT);
    expect(requests[0].path).toBe(`/${KEPT + 4}`);
  });

  it('adds what arrived in a second at once, newest first', () => {
    const requests = addAll([served('/old')], [served('/1'), served('/2')]);
    expect(requests.map((one) => one.path)).toEqual(['/2', '/1', '/old']);
    const many = Array.from({ length: KEPT + 20 }, (_, index) => served(`/${index}`));
    expect(addAll([], many)).toHaveLength(KEPT);
    expect(addAll([], many)[0].path).toBe(`/${KEPT + 19}`);
  });

  it('finds requests by host, path, method, status, client and upstream', () => {
    const one = { ...served('/api/users'), status: 503, upstream: '10.0.0.5:80' };
    for (const filter of ['', 'a.test', '/api', 'API/USERS', 'get', '503', '5xx', '127.0.0.1', '10.0.0.5', 'a.test/api 5xx']) {
      expect(matches(one, filter), filter).toBe(true);
    }
    for (const filter of ['b.test', '/web', 'post', '404', '4xx', 'a.test 2xx']) {
      expect(matches(one, filter), filter).toBe(false);
    }
  });
});
