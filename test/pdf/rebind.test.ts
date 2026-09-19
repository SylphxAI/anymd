import dns from 'node:dns';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { __setFetchUrlHopForTests, loadPdfDocument } from '../../src/pdf/loader.js';
import { __resetSecurityConfigForTests } from '../../src/utils/config.js';

/**
 * GHSA-5r2f-7788-qp8v — DNS rebinding (TOCTOU) on the URL fetch path.
 *
 * `validateUrlHop` checked the hostname, but the connection resolved it again,
 * so an attacker-controlled zone could answer the check with a public address
 * and the connect with `127.0.0.1` / `169.254.169.254`.
 *
 * The fix re-resolves the name itself and pins the approved answer into the
 * connection, so the client never resolves the name again on its own. These
 * tests assert that contract: the module resolves every hop it is about to
 * connect to, resolves it exactly twice (guard + pin), and fails closed when
 * an answer turns non-public before the connection is made.
 */
describe('URL SSRF guard against DNS rebinding', () => {
  afterEach(() => {
    __setFetchUrlHopForTests(null);
    __resetSecurityConfigForTests();
    vi.restoreAllMocks();
  });

  const stubTransport = (onHop: (url: string) => Response) => {
    const hops: string[] = [];
    __setFetchUrlHopForTests(async (url) => {
      hops.push(url);
      return onHop(url);
    });
    return hops;
  };

  const pdfResponse = () =>
    new Response(new Uint8Array([1, 2, 3]), {
      status: 200,
      headers: { 'content-type': 'application/pdf' },
    });

  it('resolves every hop itself, guard then pin, before connecting', async () => {
    const lookups: string[] = [];
    vi.spyOn(dns.promises, 'lookup').mockImplementation(async (hostname) => {
      lookups.push(String(hostname));
      return [{ address: '198.51.100.7', family: 4 }] as Awaited<
        ReturnType<typeof dns.promises.lookup>
      >;
    });
    const hops = stubTransport(() => pdfResponse());

    await loadPdfDocument(
      { url: 'http://rebind.example/doc.pdf' },
      'http://rebind.example/doc.pdf'
    ).catch(() => undefined);

    // The module resolves the host itself — guard check, then pin — and the
    // transport only runs afterwards, so nothing downstream can re-resolve it.
    expect(lookups).toEqual(['rebind.example', 'rebind.example']);
    expect(hops).toEqual(['http://rebind.example/doc.pdf']);
  }, 15000);

  it('fails closed when the pin sees a non-public answer the guard accepted', async () => {
    const lookups: string[] = [];
    // The hostile zone answers the guard with a public address and the pin with
    // loopback — the rebinding attempt, observed at the second resolution.
    vi.spyOn(dns.promises, 'lookup').mockImplementation(async (hostname) => {
      const name = String(hostname);
      lookups.push(name);
      const isFirst = lookups.filter((h) => h === name).length === 1;
      return [{ address: isFirst ? '198.51.100.7' : '127.0.0.1', family: 4 }] as Awaited<
        ReturnType<typeof dns.promises.lookup>
      >;
    });
    const hops = stubTransport(() => pdfResponse());

    await expect(
      loadPdfDocument({ url: 'http://rebind.example/doc.pdf' }, 'http://rebind.example/doc.pdf')
    ).rejects.toThrow(/non-public address|SSRF/);

    // The rebinding answer was rejected, and no transport ran against it.
    expect(lookups).toEqual(['rebind.example', 'rebind.example']);
    expect(hops).toEqual([]);
  }, 15000);

  it('refuses a hop whose only answer is the metadata endpoint, without connecting', async () => {
    vi.spyOn(dns.promises, 'lookup').mockResolvedValue([
      { address: '169.254.169.254', family: 4 },
    ] as Awaited<ReturnType<typeof dns.promises.lookup>>);
    const hops = stubTransport(() => pdfResponse());

    await expect(
      loadPdfDocument({ url: 'http://metadata.example/latest/' }, 'http://metadata.example/')
    ).rejects.toThrow(/non-public address|SSRF/);
    expect(hops).toEqual([]);
  }, 15000);

  it('re-validates and pins each redirect target, rejecting a private one', async () => {
    const lookups: string[] = [];
    vi.spyOn(dns.promises, 'lookup').mockImplementation(async (hostname) => {
      const name = String(hostname);
      lookups.push(name);
      return [
        { address: name === 'private.example' ? '127.0.0.1' : '198.51.100.7', family: 4 },
      ] as Awaited<ReturnType<typeof dns.promises.lookup>>;
    });
    const hops = stubTransport(
      () => new Response(null, { status: 302, headers: { location: 'http://private.example/x' } })
    );

    await expect(
      loadPdfDocument(
        { url: 'http://public.example/redirect.pdf' },
        'http://public.example/redirect.pdf'
      )
    ).rejects.toThrow(/non-public address|SSRF/);

    // The redirect target was resolved by this module (guard + pin) and
    // rejected; the only transport call was the first, public hop.
    expect(lookups).toEqual(['public.example', 'public.example', 'private.example']);
    expect(hops).toEqual(['http://public.example/redirect.pdf']);
  }, 15000);
});
