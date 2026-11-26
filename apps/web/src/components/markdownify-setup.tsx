import { createSignal, createUniqueId, For } from 'solid-js';
import type { CopyDetails } from '@/lib/analytics';
import { CopyButton } from './copy';

type CommandKind = Extract<CopyDetails, { surface: 'markdownify_mcp' }>['command_kind'];
const commands = [
  {
    kind: 'convert_command',
    tab: 'One-shot CLI',
    label: 'Copy command',
    text: 'npx -y @sylphx/anymd report.pdf > report.md',
  },
  {
    kind: 'cli_install',
    tab: 'Global CLI',
    label: 'Copy CLI commands',
    text: 'npm install -g @sylphx/anymd\nanymd report.pdf > report.md',
  },
  {
    kind: 'mcp_config',
    tab: 'MCP JSON',
    label: 'Copy MCP configuration',
    text: JSON.stringify({
      mcpServers: {
        anymd: {
          command: 'npx',
          args: ['-y', '@sylphx/anymd', 'mcp', '--allow-dir=/path/to/docs'],
        },
      },
    }, null, 2),
  },
  {
    kind: 'uvx',
    tab: 'Python uvx',
    label: 'Copy uvx command',
    text: 'uvx anymd report.pdf > report.md',
  },
] as const satisfies ReadonlyArray<{ kind: CommandKind; tab: string; label: string; text: string }>;

export function MarkdownifySetup() {
  const id = createUniqueId();
  const [active, setActive] = createSignal(0);
  const tabs: HTMLButtonElement[] = [];
  const tabId = (index: number) => id + '-tab-' + index;
  const panelId = (index: number) => id + '-panel-' + index;

  function navigate(event: KeyboardEvent, index: number) {
    const count = commands.length;
    let next: number;
    switch (event.key) {
      case 'ArrowLeft': next = (index + count - 1) % count; break;
      case 'ArrowRight': next = (index + 1) % count; break;
      case 'Home': next = 0; break;
      case 'End': next = count - 1; break;
      default: return;
    }
    event.preventDefault();
    setActive(next);
    tabs[next]?.focus();
  }

  return <div class="comparison-setup">
    <div role="tablist" aria-label="Setup options">
      <For each={commands}>{(command, index) => <button
        type="button"
        role="tab"
        id={tabId(index())}
        aria-controls={panelId(index())}
        aria-selected={index() === active()}
        tabIndex={index() === active() ? 0 : -1}
        ref={(button) => { tabs[index()] = button; }}
        onClick={() => setActive(index())}
        onKeyDown={(event) => navigate(event, index())}
      >{command.tab}</button>}</For>
    </div>
    <For each={commands}>{(command, index) => <div
      class="panel"
      role="tabpanel"
      id={panelId(index())}
      aria-labelledby={tabId(index())}
      tabIndex={0}
      hidden={index() !== active()}
    >
      {command.kind === 'mcp_config' && <p>Add this to a client that accepts <code>mcpServers</code>. Replace <code>/path/to/docs</code> with the folder the server may read.</p>}
      <pre class="code-box"><code>{command.text}</code></pre>
      <CopyButton text={command.text} analytics={{ surface: 'markdownify_mcp', command_kind: command.kind }} label={command.label} />
    </div>}</For>
  </div>;
}
