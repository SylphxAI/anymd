import type { Metadata } from 'next';
import { Footer, Header } from '@/components/chrome';
import { Diagnostics } from '@/components/diagnostics';
import './globals.css';

const description = 'Convert PDFs, Office documents, web pages and audio to clean Markdown. Run Anymd locally as a CLI or connect it to your agent over MCP.';
export const metadata: Metadata = {
  metadataBase: new URL('https://anymd.sylphx.com'),
  title: { default: 'Anymd — Documents in. Markdown out.', template: '%s · Anymd' },
  description,
  icons: '/favicon.svg',
  openGraph: { type: 'website', title: 'Anymd — Documents in. Markdown out.', description, images: ['/og-image.png'] },
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return <html lang="en"><body><Diagnostics /><a className="skip-link" href="#main">Skip to content</a><Header /><main id="main">{children}</main><Footer /></body></html>;
}
