import type { APIEvent } from '@solidjs/start/server';
import { translatedPaths } from '@/lib/marketing-locale';

export function GET({ request }: APIEvent) {
  return new Response(null, {
    status: 308, headers: { Location: translatedPaths['/studio'] + new URL(request.url).search },
  });
}

export const HEAD = GET;
