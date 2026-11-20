import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../../', import.meta.url));

export default {
  output: 'standalone',
  outputFileTracingRoot: root,
  poweredByHeader: false,
  async rewrites() {
    return [{ source: '/playground/sample.pdf', destination: '/samples/sample.pdf' }];
  },
  async headers() {
    return [{ source: '/:path*', headers: [
      { key: 'X-Content-Type-Options', value: 'nosniff' },
      { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
      { key: 'X-Frame-Options', value: 'DENY' },
    ] }];
  },
};
