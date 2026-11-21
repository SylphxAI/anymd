import assert from 'node:assert/strict';
import { test } from 'node:test';
import { publicOrigin } from './app-origin';
const staging = 'web-16b143.sylphx.app';
const canonical = 'https://anymd.sylphx.com';
for (const [host, forwarded, origin] of [
  [staging, '', 'https://' + staging],
  ['0.0.0.0:3000', 'WEB-16B143.SYLPHX.APP:443', 'https://' + staging],
  ['anymd.sylphx.com', '', canonical],
  ['bad.example', staging + '.bad.example', null],
  ['bad.example', staging + ', bad.example', null],
  ['other-tenant.sylphx.app', '', null],
  ['0.0.0.0:3000', staging + ':444', null],
]) {
  test('resolves ' + (forwarded || host), () => {
    const headers = new Headers({ host: String(host) });
    if (forwarded) headers.set('x-forwarded-host', forwarded);
    assert.equal(publicOrigin(headers), origin);
  });
}
