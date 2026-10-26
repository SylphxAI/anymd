// Keel engine views: start each [data-keel-embed] scene once the reader first
// interacts with the page (moves the pointer, touches, scrolls or presses a
// key) and the view is near the viewport. Until then the poster shows: a page
// with live scenes loads and settles like a static one. A view with
// data-keel-embed-start="press" never starts by itself: it shows its poster
// and a "Run" button and starts only when the reader presses it.
//
// The view holds a poster <img>. The site's scenes module, named by this
// script's data-keel-scenes attribute and imported once for every view,
// exports keel_mount(elementId, scene); the
// title fills the view with a canvas and presents through WebGPU or WebGL2.
// data-keel-embed-state tracks the view: waiting, starting, live, poster
// (no GPU output), paused (reduced motion, until the reader presses play) or
// failed. The poster stays whenever the scene is not live.

const modules = new Map();

function gpuAvailable() {
  if (navigator.gpu) return true;
  try {
    return !!document.createElement("canvas").getContext("webgl2");
  } catch {
    return false;
  }
}

// Chrome dispatches a trusted pointermove at the cursor when a page loads,
// with no input at all. It has no movement, so a move counts only when the
// pointer actually moved; a reader's first real move is followed by more.
const interacted = new Promise((resolve) => {
  const events = ["pointermove", "pointerdown", "touchstart", "wheel", "scroll", "keydown"];
  const done = (event) => {
    if (event.type === "pointermove" && !event.movementX && !event.movementY) return;
    for (const name of events) removeEventListener(name, done, { passive: true });
    resolve();
  };
  for (const name of events) addEventListener(name, done, { passive: true });
});

function nearViewport(element) {
  return new Promise((resolve) => {
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) {
          observer.disconnect();
          resolve();
        }
      },
      { rootMargin: "300px 0px" },
    );
    observer.observe(element);
  });
}

function glue(url) {
  let loading = modules.get(url);
  if (!loading) {
    loading = import(url).then(async (module) => {
      await module.default();
      return module;
    });
    modules.set(url, loading);
  }
  return loading;
}

function whenLive(view) {
  const check = () => {
    const canvas = view.querySelector("canvas");
    // The first frame reached the canvas: the poster can go without a blank frame.
    if (canvas && canvas.hasAttribute("data-keel-ready")) {
      view.setAttribute("data-keel-embed-state", "live");
      return true;
    }
    return false;
  };
  if (check()) return;
  const observer = new MutationObserver(() => check() && observer.disconnect());
  observer.observe(view, { subtree: true, childList: true, attributes: true, attributeFilter: ["data-keel-ready"] });
}

async function start(view) {
  view.setAttribute("data-keel-embed-state", "starting");
  try {
    const scenes = document.querySelector("script[data-keel-scenes]")?.getAttribute("data-keel-scenes");
    if (!scenes) throw new Error("the site has no scenes module; declare [web.scenes] in keel.toml");
    const module = await glue(new URL(scenes, document.baseURI).href);
    module.keel_mount(view.id, view.getAttribute("data-keel-embed"));
    whenLive(view);
  } catch (error) {
    view.setAttribute("data-keel-embed-state", "failed");
    console.error("keel: engine view failed to start", error);
  }
}

function playButton(view, label = "Play", description = "Play the live scene") {
  const button = document.createElement("button");
  button.type = "button";
  button.setAttribute("data-keel-embed-play", "");
  // The theme's primary button surface styles it.
  button.setAttribute("data-keel-surface", "btn-primary");
  button.className = "keel-surface-btn-primary";
  button.textContent = label;
  button.setAttribute("aria-label", description);
  button.addEventListener("click", () => {
    button.remove();
    start(view);
  });
  view.append(button);
}

async function prepare(view) {
  view.setAttribute("data-keel-embed-state", "waiting");
  if (!gpuAvailable()) {
    view.setAttribute("data-keel-embed-state", "poster");
    return;
  }
  if (view.getAttribute("data-keel-embed-start") === "press") {
    const alt = view.querySelector("img")?.getAttribute("alt");
    view.setAttribute("data-keel-embed-state", "paused");
    playButton(view, "Run", alt ? `Run: ${alt}` : "Run");
    return;
  }
  if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
    view.setAttribute("data-keel-embed-state", "paused");
    playButton(view);
    return;
  }
  await interacted;
  await nearViewport(view);
  await start(view);
}

for (const view of document.querySelectorAll("[data-keel-embed]")) {
  if (!view.hasAttribute("data-keel-embed-state")) prepare(view);
}
