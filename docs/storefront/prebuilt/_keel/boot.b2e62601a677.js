// One client per document; input remains usable while its module loads.
const c = document.querySelector('script[data-keel-client]');
const lazy = c.hasAttribute('data-keel-lazy');
const live = () => window.__keel_hydrated === true;
/** @type {'idle' | 'loading' | 'ready' | 'error'} */
let state = 'idle';
let flight, preparation, client, status, message, retry, observer, previousFocus, retryFocus, reloadRequired;
const held = [];
const busy = new Map();
const affected = new Set();

const module = () => client ||= import(c.dataset.keelClient).catch(error => {
  client = null;
  throw error;
});

function prepare() {
  if (preparation || flight || live()) return;
  preparation = module().then(client => client.keel_prepare_client?.()).catch(error => {
    preparation = null;
    // A background failure is retried on direct intent. No unhandled rejection
    // and no initialization or replay happens merely because the page is idle.
    console.warn('Interactive content could not be prepared. Interaction will retry loading.', error);
  });
}

function restoreBusy() {
  for (const [element, value] of busy) {
    if (value === null) element.removeAttribute('aria-busy');
    else element.setAttribute('aria-busy', value);
  }
  busy.clear();
}

function feedback(root) {
  if (root) affected.add(root);
  if (root && !busy.has(root)) {
    busy.set(root, root.getAttribute('aria-busy'));
    root.setAttribute('aria-busy', 'true');
  }
  if (!status) {
    previousFocus = document.activeElement;
    status = document.createElement('aside');
    status.dataset.keelClientStatus = '';
    status.setAttribute('role', 'status');
    status.setAttribute('aria-live', 'polite');
    status.setAttribute('aria-atomic', 'true');
    message = document.createElement('span');
    retry = document.createElement('button');
    retry.type = 'button';
    retry.addEventListener('click', () => reloadRequired ? location.reload() : go());
    status.append(message, retry);
    document.body.append(status);
  }
  status.hidden = false;
  message.textContent = reloadRequired
    ? 'Interactive content did not finish starting. Reload the page to try again.'
    : state === 'error'
    ? 'Interactive content could not load. Check your connection and retry.'
    : 'Loading interactive content…';
  retry.hidden = state !== 'error';
  retry.textContent = reloadRequired ? 'Reload' : 'Retry';
}

function ready() {
  state = c.dataset.keelClientState = 'ready';
  observer?.disconnect();
  removeEventListener('click', activate, true);
  for (const type of ['pointerdown', 'keydown', 'touchstart', 'focusin'])
    removeEventListener(type, intent, true);
  restoreBusy();
  affected.clear();
  if (status) {
    // Returning from Retry never leaves keyboard focus on a removed button.
    if (document.activeElement === retry || (retryFocus && document.activeElement === document.body)) {
      const focus = held[0]?.target.closest('button,[tabindex]') || previousFocus;
      if (focus?.isConnected) focus.focus({preventScroll:true});
    }
    status.remove();
    status = null;
  }
  const actions = held.splice(0);
  for (const {target, event} of actions) {
    if (target.isConnected) target.dispatchEvent(new MouseEvent('click', event));
  }
}

function waitForHydration() {
  if (live()) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const started = performance.now();
    const timer = setInterval(() => {
      if (live()) { clearInterval(timer); resolve(); }
      else if (performance.now() - started >= 30000) {
        clearInterval(timer);
        reloadRequired = true;
        reject(new Error('The client did not finish hydration. Reload the page to try again.'));
      }
    }, 16);
  });
}

function go() {
  if (live()) { ready(); return; }
  if (flight || reloadRequired) return;
  retryFocus = document.activeElement === retry;
  state = c.dataset.keelClientState = 'loading';
  for (const root of affected) if (root.isConnected) feedback(root);
  if (status) feedback();
  // A stalled import or initializer must also offer a visible way forward.
  // It may still complete, so never start a second initializer on timeout.
  const deadline = setTimeout(() => {
    if (live()) { ready(); return; }
    reloadRequired = true;
    state = c.dataset.keelClientState = 'error';
    restoreBusy();
    feedback();
  }, 30000);
  flight = module()
    .then(async client => {
      const compiled = preparation ? await preparation : undefined;
      // Preparation can make initialization synchronous on the next intent.
      // Give the loading status a rendering opportunity before starting it.
      if (status) {
        await new Promise(requestAnimationFrame);
        await new Promise(requestAnimationFrame);
      }
      return compiled ? client.default({module_or_path:compiled}) : client.default();
    })
    .then(waitForHydration)
    .then(ready)
    .catch(error => {
      state = c.dataset.keelClientState = 'error';
      restoreBusy();
      feedback();
      console.error('Interactive content could not load. Retry or reload the page.', error);
    })
    .finally(() => { clearTimeout(deadline); flight = null; });
}

function scope(event) {
  if (!(event.target instanceof Element) || status?.contains(event.target)) return null;
  return lazy ? event.target.closest('[data-keel-island]')
    : event.target.closest('#keel-static-root');
}

function intent(event) {
  if (live()) return;
  const root = scope(event);
  if (!root) return;
  feedback(root);
  go();
}

function activate(event) {
  if (live()) return;
  const root = scope(event);
  if (!root) return;
  // Links, disclosures, fields and form submissions already have a native
  // action. Repeating it after hydration would activate them twice.
  const native = event.target.closest('a[href],summary,input,select,textarea,label');
  const button = event.target.closest('button');
  const submits = button?.form && button.type !== 'button';
  if (!native && !submits && !event.defaultPrevented) {
    held.push({target:event.target, event:{bubbles:true,cancelable:true,
      ctrlKey:event.ctrlKey,altKey:event.altKey,shiftKey:event.shiftKey,
      metaKey:event.metaKey,button:event.button,buttons:event.buttons,detail:event.detail}});
    // A client may attach listeners before it declares hydration complete.
    // Only the replay may reach them while this activation is queued.
    event.preventDefault();
    event.stopImmediatePropagation();
  }
  intent(event);
}
addEventListener('click', activate, true);
for (const type of ['pointerdown', 'keydown', 'touchstart', 'focusin']) {
  addEventListener(type, intent, {capture:true,passive:true});
}
c.dataset.keelClientState = state;
if (lazy) {
  const connection = navigator.connection;
  const intentOnly = connection?.saveData || ['slow-2g', '2g'].includes(connection?.effectiveType);
  if (!intentOnly && typeof IntersectionObserver === 'function') {
    observer = new IntersectionObserver(entries => {
      if (entries.some(entry => entry.isIntersecting)) {
        observer.disconnect();
        go();
      }
    }, {rootMargin:'200px'});
    document.querySelectorAll('[data-keel-island],[data-keel-island]>*')
      .forEach(element => observer.observe(element));
  }
  if (!intentOnly) {
    const idle = () => requestAnimationFrame(() => {
      const start = () => {
        if (state === 'idle' && document.querySelector('[data-keel-island]')) prepare();
      };
      if (typeof requestIdleCallback === 'function') requestIdleCallback(start);
      else setTimeout(start, 0);
    });
    if (document.readyState === 'complete') idle();
    else addEventListener('load', idle, {once:true});
  }
} else go();
