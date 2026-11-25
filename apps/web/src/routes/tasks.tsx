import { ArrowRight } from 'lucide-solid';
import { For } from 'solid-js';
import { PageMeta } from '@/components/page-meta';
import { collectionPageSchema, StructuredData } from '@/components/search-details';
import { CampaignLink } from '@/lib/attribution-link';

const guides = [
  { href: '/tasks/check-pdf-citation', title: 'Check a PDF citation', description: 'Verify that a quote appears on the cited page and in the cited area.' },
  { href: '/tasks/build-video-timeline', title: 'Build a video timeline', description: 'Find scene changes and timestamped cues in a local video.' },
  { href: '/tasks/extract-video-frame', title: 'Extract a video frame', description: 'Capture an exact frame and record its decoded timestamp and hash.' },
] as const;

const path = '/tasks';
const title = 'Task guides';

export default function TasksRoute() {
  return <section class="wrap page-shell">
    <PageMeta title={title} canonical={path} description="Step-by-step Anymd guides for verifying PDF quotes, building video timelines, and citing exact video frames." />
    <StructuredData nodes={[collectionPageSchema(path, title, guides)]} />
    <header class="page-heading">
      <span class="eyebrow">DOCUMENTATION</span>
      <h1>{title}</h1>
      <p>Follow a worked example to verify a PDF quote or find the exact moment in a video.</p>
    </header>
    <ul class="stack max-w-3xl">
      <For each={guides}>{(guide) => <li>
        <CampaignLink href={guide.href} class="panel flex items-center justify-between gap-4 hover:border-[var(--accent)]">
          <div><h2>{guide.title}</h2><p class="muted mt-2">{guide.description}</p></div>
          <ArrowRight size={20} aria-hidden="true" class="shrink-0 text-[var(--accent)]" />
        </CampaignLink>
      </li>}</For>
    </ul>
    <p class="mt-6"><CampaignLink class="inline-link" href="/docs">Back to documentation</CampaignLink></p>
  </section>;
}
