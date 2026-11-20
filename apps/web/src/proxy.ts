import { type NextRequest, NextResponse } from 'next/server';
import { isCanonicalHost, noindexTag } from './lib/indexing';

export function proxy(request: NextRequest) {
  const response = NextResponse.next();
  if (!isCanonicalHost(request.headers)) response.headers.set('X-Robots-Tag', noindexTag);
  return response;
}
