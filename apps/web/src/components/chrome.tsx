import { ArrowUpRight, Braces, Menu } from 'lucide-react';
import { GithubMark } from "./github-mark";
import Link from 'next/link';
import { SessionAction } from './session';

const links = [['Studio', '/studio'], ['Docs', '/docs'], ['Pricing', '/pricing']] as const;

export function Header() {
  return (
    <header className="site-header">
      <div className="wrap header-row">
        <Link href="/" className="brand" aria-label="Anymd home"><span><Braces size={20} strokeWidth={2.4} /></span>anymd<span className="brand-dot">.</span></Link>
        <nav aria-label="Main" className="main-nav">{links.map(([text, href]) => <Link key={href} href={href}>{text}</Link>)}</nav>
        <div className="header-actions">
          <a href="https://github.com/SylphxAI/anymd" className="github-header" aria-label="Anymd GitHub repository"><GithubMark size={18} /></a>
          <SessionAction />
          <details className="mobile-menu"><summary aria-label="Open navigation"><Menu size={22} /></summary><nav aria-label="Mobile">{links.map(([text, href]) => <a key={href} href={href}>{text}</a>)}</nav></details>
        </div>
      </div>
    </header>
  );
}

export function Footer() {
  return (
    <footer className="site-footer"><div className="wrap footer-row">
      <div><Link className="brand" href="/"><span><Braces size={18} /></span>anymd<span className="brand-dot">.</span></Link><p>Documents in. Markdown out.</p></div>
      <div className="footer-links"><Link href="/docs">Docs</Link><Link href="/vs">Comparisons</Link><Link href="/recover">Recover purchase</Link><Link href="/legal/privacy">Privacy</Link><Link href="/legal/pro-terms">Pro terms</Link><a href="https://github.com/SylphxAI/anymd">GitHub <ArrowUpRight size={13} /></a></div>
    </div></footer>
  );
}
