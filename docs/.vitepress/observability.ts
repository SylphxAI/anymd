const url =
  'https://api.sylphx.com/v1/orgs/org_fs01afbdccgn8xdk1ghj5y47w0vtph/projects/prj_fs013ejakyn22t4bsvrvyj0f2x1pgq/envs/env_fs01p2f389mwmm2xsm1nmvcst69vgd/error_groups:capture';

const headStart = /<head>\s*<meta charset="utf-8">/;

export function injectObservability(html: string, page: string, env: NodeJS.ProcessEnv): string {
  // VitePress invokes this hook on rendered pages, including its synthetic 404.
  if (!page.endsWith('.md') || /^(?:404|pro|pricing|recover|storefront)(?:[/.]|$)/.test(page)) {
    return html;
  }
  const head = headStart.exec(html);
  if (!head) return html;

  const key = env.ANYMD_OBS_PUBLISHABLE_KEY;
  const release = env.GITHUB_SHA;
  if (
    env.ANYMD_OBS_CAPTURE_URL !== url ||
    !key ||
    !/^sylphx_pk_live_[a-z0-9]{4}[A-Za-z0-9]{16}$/.test(key) ||
    !release ||
    !/^[0-9a-f]{40}$/.test(release)
  ) {
    return html;
  }
  // The first classic script must register capture listeners before page assets.
  const insert = head.index + head[0].length;
  const script = `<script src="/anymd/obs.js" data-capture-url="${url}" data-publishable-key="${key}" data-release="${release}"></script>`;
  return html.slice(0, insert) + '\n    ' + script + html.slice(insert);
}
