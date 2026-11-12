import type { NextConfig } from 'next';
import path from 'node:path';
const config: NextConfig = {
  poweredByHeader: false,
  devIndicators: false,
  // Use TypeScript's compiler API; detached CLI subprocess output is restricted
  // by the local executor. This keeps build-time type checking fully enabled.
  experimental: { useTypeScriptCli: false, cpus: 2 },
  outputFileTracingRoot: path.resolve(process.cwd()),
  async headers() {
    return [{ source: '/:path*', headers: [
      { key: 'X-Content-Type-Options', value: 'nosniff' },
      { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
      { key: 'X-Frame-Options', value: 'SAMEORIGIN' },
      { key: 'Permissions-Policy', value: 'camera=(), microphone=(), geolocation=()' },
    ] }];
  },
};
export default config;
