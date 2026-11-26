import { expect, test, type Locator } from '@playwright/test';

const campaign = 'utm_source=comparison&utm_campaign=PDF+Proof';
const setups = [
  ['One-shot CLI', 'Copy command', 'npx -y @sylphx/anymd report.pdf > report.md'],
  ['Global CLI', 'Copy CLI commands', 'npm install -g @sylphx/anymd\nanymd report.pdf > report.md'],
  ['MCP JSON', 'Copy MCP configuration', JSON.stringify({
    mcpServers: { anymd: { command: 'npx', args: ['-y', '@sylphx/anymd', 'mcp', '--allow-dir=/path/to/docs'] } },
  }, null, 2)],
  ['Python uvx', 'Copy uvx command', 'uvx anymd report.pdf > report.md'],
];
const examples = [
  { tab: 'PDF', heading: 'River survey', key: 'pdf', cta: 'Convert sample.pdf in Studio', needle: '<!-- page 3 -->' },
  { tab: 'Multi-page document', heading: 'Field brief', key: 'document', cta: 'Convert brief.html in Studio', needle: '<!-- slide 2 -->' },
  { tab: 'Video frame', heading: 'Frame citation', key: 'video', cta: 'Open demo.mp4 in Studio', needle: 'Decoded: 00:01.100' },
];

const categoryHeadings = ['Install', 'Documents', 'Images and media', 'Web content', 'Markdown output'];
const toolHeadings = ['Anymd', 'Markdownify MCP'];

test('comparison cards preserve all headings, labels and table roles at 390px', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto('/vs/markdownify-mcp');
  const table = page.getByRole('table', { name: 'How the two tools compare' });
  await expect(table.getByRole('columnheader')).toHaveText(['What to check', ...toolHeadings]);
  expect(await table.locator('thead').evaluate(head => getComputedStyle(head).clipPath)).toBe('inset(50%)');
  const rows = table.getByRole('row').filter({ has: page.getByRole('rowheader') });
  await expect(rows).toHaveCount(categoryHeadings.length);
  await expect(table.getByRole('rowheader')).toHaveText(categoryHeadings);

  for (const [i, heading] of categoryHeadings.entries()) {
    const row = rows.nth(i);
    const title = row.getByRole('rowheader', { name: heading, exact: true });
    const splitWords = await title.evaluate(header => {
      const text = header.firstChild!;
      const range = document.createRange();
      // Check painted line fragments for every word, not just the heading box.
      return [...(text.textContent ?? '').matchAll(/\S+/g)].filter(({ 0: word, index }) => {
        range.setStart(text, index);
        range.setEnd(text, index + word.length);
        return range.getClientRects().length !== 1;
      }).map(([word]) => word);
    });
    expect(splitWords, heading).toEqual([]);
    const titleBox = (await title.boundingBox())!;
    let bottom = titleBox.y + titleBox.height;
    const cells = row.getByRole('cell');
    await expect(cells).toHaveCount(toolHeadings.length);
    for (const [j, label] of toolHeadings.entries()) {
      const cell = cells.nth(j);
      await expect(cell).toHaveAttribute('data-label', label);
      const pseudo = await cell.evaluate(node => {
        const style = getComputedStyle(node, '::before');
        return { content: style.content, display: style.display };
      });
      expect(pseudo).toEqual({ content: JSON.stringify(label) + ' / ""', display: 'block' });
      const box = (await cell.boundingBox())!;
      expect(box.x).toBe(titleBox.x);
      expect(box.width).toBe(titleBox.width);
      expect(box.y).toBeGreaterThanOrEqual(bottom);
      bottom = box.y + box.height;
    }
  }
  expect(await page.evaluate(() => ({ viewport: innerWidth, scroll: document.documentElement.scrollWidth })))
    .toEqual({ viewport: 390, scroll: 390 });
});

test('comparison remains a native three-column table above 660px', async ({ page }) => {
  await page.setViewportSize({ width: 661, height: 900 });
  await page.goto('/vs/markdownify-mcp');
  const table = page.getByRole('table', { name: 'How the two tools compare' });
  expect(await table.evaluate(node => [node.tagName, getComputedStyle(node).display])).toEqual(['TABLE', 'table']);
  const columns = table.getByRole('columnheader');
  await expect(columns).toHaveText(['What to check', ...toolHeadings]);
  await expect(table.locator('thead th[scope="col"]')).toHaveCount(3);
  await expect(table.locator('tbody th[scope="row"]')).toHaveText(categoryHeadings);
  await expect(table.getByRole('rowheader')).toHaveText(categoryHeadings);
  const [title, anymd, markdownify] = await Promise.all((await columns.all()).map(header => header.boundingBox()));
  expect(title!.y).toBe(anymd!.y);
  expect(anymd!.y).toBe(markdownify!.y);
  expect(title!.x).toBeLessThan(anymd!.x);
  expect(anymd!.x).toBeLessThan(markdownify!.x);
  expect(await table.getByRole('cell').first().evaluate(node => getComputedStyle(node, '::before').content)).toBe('none');
});

test('comparison setup and format proof lead to a local Studio conversion before consent', async ({ page, context, hasTouch }) => {
  const errors: string[] = [];
  const signal: string[] = [];
  await context.grantPermissions(['clipboard-read', 'clipboard-write']);
  await page.route('**/cdn-cgi/trace', route => route.fulfill({ body: 'loc=DE\n' }));
  await page.addInitScript(() => document.addEventListener('securitypolicyviolation', event => {
    console.error('CSP violation: ' + event.violatedDirective + ' ' + event.blockedURI);
  }));
  page.on('pageerror', error => errors.push(error.message));
  page.on('console', message => {
    if (message.type() === 'error' || /content security policy/i.test(message.text())) errors.push(message.text());
  });
  context.on('request', request => {
    if (new URL(request.url()).pathname === '/v1/analytics/events:batch') signal.push(request.url());
  });
  const activate = (control: Locator) => hasTouch ? control.tap() : control.click();
  const clipboard = () => page.evaluate(() => navigator.clipboard.readText());
  const noOverflow = async () => expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(page.viewportSize()!.width);
  const nonce = /script-src [^;]*'nonce-[^']+'/;
  const response = await page.goto('/vs/markdownify-mcp?' + campaign + '&sample=video&ignored=private');
  expect(response?.ok()).toBe(true);
  expect(response?.headers()['content-security-policy']).toMatch(nonce);
  await expect(page.getByRole('button', { name: 'Reject optional' })).toBeEnabled();
  await expect(page.getByRole('link', { name: 'See Anymd Pro' })).toHaveAttribute('href', '/pricing?entry_surface=seo_vs-markdownify-mcp&' + campaign);

  const setup = page.locator('#install');
  const oneShot = setup.getByRole('tab', { name: 'One-shot CLI' });
  await expect(oneShot).toHaveAttribute('aria-selected', 'true');
  await oneShot.focus();
  await page.keyboard.press('ArrowLeft');
  await expect(setup.getByRole('tab', { name: 'Python uvx' })).toBeFocused();
  await expect(setup.getByRole('tabpanel', { name: 'Python uvx' })).toBeVisible();
  await page.keyboard.press('Home');
  await expect(oneShot).toBeFocused();
  for (const [tab, label, text] of setups) {
    const selected = setup.getByRole('tab', { name: tab, exact: true });
    await activate(selected);
    await expect(selected).toHaveAttribute('aria-selected', 'true');
    await expect(setup.getByRole('tabpanel')).toHaveCount(1);
    const panel = setup.getByRole('tabpanel', { name: tab, exact: true });
    expect(await panel.locator('pre code').textContent()).toBe(text);
    await activate(panel.getByRole('button', { name: label, exact: true }));
    await expect(panel.getByRole('button', { name: 'Copied', exact: true })).toBeVisible();
    await expect.poll(clipboard).toBe(text);
    await noOverflow();
  }

  const demo = page.getByRole('region', { name: 'Conversion examples' });
  const panel = demo.getByRole('tabpanel');
  for (const { tab, heading, key, cta, needle } of examples) {
    await activate(demo.getByRole('tab', { name: tab, exact: true }));
    const raw = panel.locator('pre code');
    await expect(raw).toContainText(needle);
    const text = await raw.textContent();
    await activate(panel.getByRole('button', { name: 'Preview', exact: true }));
    await expect(panel.getByRole('heading', { name: heading, exact: true })).toBeVisible();
    await activate(panel.getByRole('button', { name: 'Copy Markdown' }));
    await expect(demo.getByRole('status')).toHaveText('Markdown copied.');
    await expect.poll(clipboard).toBe(text);
    await expect(panel.getByRole('link', { name: cta })).toHaveAttribute('href', '/studio?sample=' + key + '&' + campaign);
    await noOverflow();
    await activate(panel.getByRole('button', { name: 'Markdown', exact: true }));
    expect(await raw.textContent()).toBe(text);
  }
  await expect(page.locator('article.comparison-page [style]')).toHaveCount(0);
  await activate(demo.getByRole('tab', { name: 'PDF', exact: true }));
  const pdf = page.waitForResponse(res => new URL(res.url()).pathname === '/samples/sample.pdf');
  await activate(panel.getByRole('link', { name: 'Convert sample.pdf in Studio' }));
  await expect(page).toHaveURL('/studio?sample=pdf&' + campaign);
  expect((await pdf).ok()).toBe(true);
  const studio = page.locator('.studio');
  await expect(studio.locator('.studio-controls')).toHaveAttribute('aria-busy', 'false');
  await expect(studio.locator('.studio-label')).toContainText('sample.pdf');
  await expect(studio.locator('.studio-code')).toContainText('Quarterly Field Report');
  await noOverflow();
  expect(signal).toEqual([]);
  expect(errors).toEqual([]);
});
