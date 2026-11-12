import type { Metadata, Viewport } from 'next';
import { PortalShell } from '@/components/ui-system/PortalShell';
import 'katex/dist/katex.min.css';
import './globals.css';
import './research-theme.css';
export const metadata: Metadata = { title: { default: 'Statis — Research Portal', template: '%s · Statis' }, description: 'Seed-deterministic userspace supervision for Linux x86-64. Inspect the implementation, fixtures, paper, and benchmark record by Siddid Pandey.', icons: { icon: '/statis-mark.svg', shortcut: '/statis-mark.svg' }, robots: { index: true, follow: true }, openGraph: { title: 'Statis Research Portal', description: 'Selected inputs. Repeatable seeds. Inspectable limits.', type: 'website' } };
export const viewport: Viewport = { themeColor: '#f5f3ed', width: 'device-width', initialScale: 1 };
export default function RootLayout({ children }: {
    children: React.ReactNode;
}) { return <html lang="en"><body id="top"><a className="skip-link" href="#main-content">Skip to content</a><PortalShell>{children}</PortalShell><noscript><style>{'.orbit-preloader{display:none!important}.pixel-band i{opacity:0!important}'}</style></noscript></body></html>; }
