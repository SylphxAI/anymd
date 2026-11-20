// Failure-only. No document content, raw errors, URLs, or persistent state.
(() => {
  'use strict';
  const config = document.currentScript?.dataset;
  const parent = 'orgs/org_fs01afbdccgn8xdk1ghj5y47w0vtph/projects/prj_fs013ejakyn22t4bsvrvyj0f2x1pgq/envs/env_fs01p2f389mwmm2xsm1nmvcst69vgd';
  const url = 'https://api.sylphx.com/v1/' + parent + '/error_groups:capture';
  const onDocs = () =>
    location.origin === 'https://anymd.sylphx.com' &&
    /^\/(?:$|(?:docs|guide|vs|use-cases|tasks|studio|playground)(?:[/.]|$))/.test(location.pathname);
  if (!onDocs() || config?.captureUrl !== url ||
      !/^sylphx_pk_live_[a-z0-9]{4}[A-Za-z0-9]{16}$/.test(config.publishableKey) ||
      !/^[0-9a-f]{40}$/.test(config.release)) return;

  const messages = {
    Exception: 'Uncaught browser exception',
    Rejection: 'Unhandled promise rejection',
    BrokenAsset: 'Page asset did not load',
    DeadClick: 'Repeated clicks without a visible action',
  };
  const assetTypes = new Set(['IMG', 'AUDIO', 'VIDEO', 'SOURCE', 'SCRIPT', 'LINK']);
  const controls = 'a[href],button,input,select,[role=button]';
  const crumbs = [], attempts = [], sent = new Set();
  function note(category, message) {
    if (crumbs.length === 15) crumbs.shift();
    crumbs.push({ category, message, event_time: new Date().toISOString() });
  }
  const surface = () => /^\/(?:playground|studio)(?:\.html|\/)?$/.test(location.pathname) ? 'playground' : 'docs';
  function navigation() {
    clearBurst();
    if (onDocs()) note('navigation', surface());
    else crumbs.length = 0;
  }
  function report(kind, component) {
    if (!onDocs()) return;
    const page = surface();
    const fingerprint = ['anymd-docs', kind, page, component].join(':');
    const now = performance.now();
    while (attempts.length && now - attempts[0] >= 60000) attempts.shift();
    if (sent.has(fingerprint) || attempts.length >= 5) return;
    const body = JSON.stringify({ parent, error_event: {
      event_time: new Date().toISOString(), exception_type: kind, message: messages[kind],
      service_name: 'anymd-docs', release: config.release, fingerprint,
      tags: { surface: page, component }, breadcrumbs: crumbs,
    } });
    if (new TextEncoder().encode(body).byteLength > 32768) return;
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), 2500);
    sent.add(fingerprint);
    attempts.push(now);
    Promise.resolve().then(() => capture(url, {
      method: 'POST', body, credentials: 'omit', referrerPolicy: 'no-referrer',
      redirect: 'error', signal: controller.signal,
      headers: { 'Content-Type': 'application/json', Authorization: 'Bearer ' + config.publishableKey },
    })).then(() => clearTimeout(timeout), () => clearTimeout(timeout));
  }
  const capture = window.fetch.bind(window);
  window.addEventListener('anymd:render-error', () => report('Exception', 'window'));
  window.addEventListener('error', event => {
    if (assetTypes.has(event.target?.tagName)) report('BrokenAsset', event.target.tagName.toLowerCase());
    else if (event.target === window) report('Exception', 'window');
  }, true);
  window.addEventListener('unhandledrejection', () => report('Rejection', 'window'));
  const emptySrc = 'img[src=""]:not([srcset]),audio[src=""],video[src=""]';
  function checkEmptySrc(node) {
    if (!(node instanceof Element)) return;
    if (node.matches(emptySrc)) report('BrokenAsset', node.tagName.toLowerCase());
    for (const el of node.querySelectorAll(emptySrc)) report('BrokenAsset', el.tagName.toLowerCase());
  }
  document.addEventListener('DOMContentLoaded', () => {
    checkEmptySrc(document.body);
    new MutationObserver(records => {
      if (!onDocs()) return;
      for (const record of records) {
        if (record.type === 'attributes') {
          const el = record.target;
          if (el.matches(emptySrc)) report('BrokenAsset', el.tagName.toLowerCase());
        } else {
          for (const node of record.addedNodes) checkEmptySrc(node);
        }
      }
    }).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['src', 'srcset'] });
  }, { once: true });

  let burst;
  function clearBurst() {
    if (!burst) return;
    clearTimeout(burst.timer);
    burst.observer.disconnect();
    burst = undefined;
  }
  window.addEventListener('pagehide', clearBurst);
  window.addEventListener('hashchange', navigation);
  window.addEventListener('anymd:navigation', navigation);
  window.addEventListener('anymd:sample-http', event => {
    const status = event.detail;
    if (onDocs() && Number.isInteger(status) && status >= 100 && status <= 599) {
      note('http', 'GET playground sample: ' + status);
    }
  });
  navigation();

  // Observe request starts, including requests still pending at the end of a
  // click burst. Never inspect request arguments or wrap the returned Promise.
  window.fetch = new Proxy(window.fetch, {
    apply(target, receiver, args) {
      clearBurst();
      return Reflect.apply(target, receiver, args);
    },
  });
  XMLHttpRequest.prototype.send = new Proxy(XMLHttpRequest.prototype.send, {
    apply(target, receiver, args) {
      clearBurst();
      return Reflect.apply(target, receiver, args);
    },
  });
  const resourceTiming = typeof PerformanceObserver !== 'undefined' &&
    PerformanceObserver.supportedEntryTypes?.includes('resource');
  if (resourceTiming) {
    new PerformanceObserver(list => {
      if (burst && list.getEntries().some(entry => entry.startTime >= burst.started)) clearBurst();
    }).observe({ type: 'resource' });
  }
  document.addEventListener('submit', clearBurst, true);
  document.addEventListener('click', event => {
    if (!onDocs() || !event.isTrusted || event.button !== 0 || event.altKey || event.ctrlKey ||
        event.shiftKey || event.metaKey || !(event.target instanceof Element)) return;
    const node = event.target.closest(controls);
    if (!node || node.matches(':disabled,[aria-disabled=true]')) return clearBurst();
    const label = ['A', 'BUTTON', 'INPUT', 'SELECT'].includes(node.tagName) ? node.tagName.toLowerCase() : 'button';
    note('ui.click', label);
    if (!resourceTiming || !node.matches('button:not([type=submit]),[role=button]:not(a)') ||
        node.form || node.hasAttribute('data-obs-download')) return clearBurst();
    const control = node.getAttribute('data-obs-control');
    const component = ['copy', 'playground-sample', 'rendered-tab', 'raw-tab'].includes(control) ? control : label;
    const now = performance.now();
    if (burst && now - burst.started > 1200) clearBurst();
    if (!burst || node !== burst.node) {
      clearBurst();
      const observer = new MutationObserver(clearBurst);
      observer.observe(document.body, { attributes: true, childList: true, characterData: true, subtree: true });
      burst = { node, observer, count: 0, started: now, href: location.href, timer: setTimeout(clearBurst, 1200) };
    }
    if (++burst.count === 3) {
      clearTimeout(burst.timer);
      burst.timer = setTimeout(() => {
        if (node.isConnected && burst.href === location.href && !burst.observer.takeRecords().length) {
          report('DeadClick', component);
        }
        clearBurst();
      }, 200);
    }
  }, true);
})();
