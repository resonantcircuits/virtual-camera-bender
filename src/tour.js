// First-run tour: a spotlight overlay that walks new users through the main
// sections of the UI, one card at a time. Shown automatically once (tracked
// in localStorage) and replayable from the help dialog. Sections that live
// inside a collapsed <details> are expanded while their step is active and
// restored when the tour moves on.

const STORAGE_KEY = "vcb.tour";

const STEPS = [
  {
    target: null,
    title: "Welcome to the Virtual Camera Bender",
    body:
      "This is a broken-camera simulator. Load a photo or video and it gets " +
      "re-shot through a virtual camera you are free to damage — bent " +
      "circuits, dying sensors, corrupt memory. This short tour points out " +
      "the main controls. Click Next to continue, or Esc to skip."
  },
  {
    target: ".preset-gallery-section",
    title: "Camera Presets",
    body:
      "Ready-made broken cameras. Clicking one copies its full settings into " +
      "the editor as a starting point — nothing is locked, every control " +
      "still works from there. Gallery (G) previews all of them on your own " +
      "image at once."
  },
  {
    target: ".randomizer-section",
    title: "Randomize",
    body:
      "New Random Camera composes a whole new damage chain from scratch. The " +
      "smaller buttons re-roll only one kind of damage (color, melt, noise…) " +
      "and keep the rest — a safe way to refine a promising camera. " +
      "Intensity sets how hard everything hits, from Gentle to Wrecked, and " +
      "the Seed moves the damage to new places without changing settings."
  },
  {
    target: "#sensorEffectsSection",
    title: "Sensor Effects",
    body:
      "Faults inside the camera's electronics — glitched clocks, bent signal " +
      "paths — hitting the raw sensor signal before the image even exists. " +
      "These run before everything else and make the most hardware-looking " +
      "damage."
  },
  {
    target: '[data-module-key="busBend"]',
    title: "Try: Bus Bend",
    body:
      "The classic circuit bend, in software: patch wires shorting bits of " +
      "the sensor's data bus together, through a filter pot and a series " +
      "resistor. Most of the traditional bent-camera looks — solarized " +
      "ghosts, screaming false color, edge-triggered streaks — come from " +
      "this one module. Flip a Source DIP switch and raise Inject Strength " +
      "to start."
  },
  {
    target: "#imageEffectsSection",
    title: "Image Effects",
    body:
      "The heart of the app: one module per effect — color faults, melt, " +
      "burn, noise, memory corruption, codec crunch. The lamp switches a " +
      "module on or off, the dice re-rolls just that module, the eye solos " +
      "it, and the lock protects it from Randomize. Hover anything for a " +
      "short description."
  },
  {
    target: '[data-module-key="colorBend"]',
    title: "Try: Color Bend",
    body:
      "One module with a huge range: hue rotation, hard channel swaps, " +
      "per-channel inversion, and solarize — each with its own strength " +
      "slider, so it goes from a subtle color cast to a full psychedelic " +
      "negative. A good first module to learn the controls on."
  },
  {
    target: "#classicEditSection",
    title: "Classic Adjustments",
    body:
      "Ordinary photo edits — brightness, contrast, saturation, temperature " +
      "— applied at the very end of the chain. Use them to polish the " +
      "finished result; Randomize leaves them alone."
  },
  {
    target: "#cameraFileGroup",
    title: "Saving Cameras",
    body:
      "Found a look worth keeping? Save Camera writes the whole damage " +
      "chain — every module, parameter, and seed — to a small file you can " +
      "bring back later with Load Camera, or share with someone else. Your " +
      "image is never stored in it."
  },
  {
    target: ".export-cluster",
    title: "Exporting",
    body:
      "Export Image saves the bent result as PNG, WebP, or JPEG. Preview " +
      "size exports exactly what you see — the safe default, since many " +
      "effects are resolution-dependent. Full re-renders at the original " +
      "source resolution instead."
  },
  {
    target: null,
    title: "Start bending",
    body:
      "Drop in an image, or try the bundled sample. Hold C to peek at the " +
      "original, press S for a split compare. The ? button up top opens the " +
      "full manual whenever you need it."
  }
];

let layer = null;
let spotlight = null;
let card = null;
let stepIndex = 0;
let active = false;
let openedDetails = [];

export function maybeStartTour() {
  try {
    if (localStorage.getItem(STORAGE_KEY) === "done") return;
  } catch {
    // storage unavailable (private mode); still show the tour this session
  }
  startTour();
}

export function startTour() {
  if (active) return;
  active = true;
  stepIndex = 0;
  buildLayer();
  document.addEventListener("keydown", onKeyDown, true);
  window.addEventListener("resize", reposition);
  showStep(0);
}

function endTour() {
  if (!active) return;
  active = false;
  restoreDetails();
  document.removeEventListener("keydown", onKeyDown, true);
  window.removeEventListener("resize", reposition);
  layer?.remove();
  layer = null;
  spotlight = null;
  card = null;
  try {
    localStorage.setItem(STORAGE_KEY, "done");
  } catch {
    // ignore
  }
}

function buildLayer() {
  layer = document.createElement("div");
  layer.className = "tour-layer";

  spotlight = document.createElement("div");
  spotlight.className = "tour-spotlight";

  card = document.createElement("div");
  card.className = "tour-card";
  card.innerHTML = `
    <span class="tour-kicker">TOUR</span>
    <h3 class="tour-title"></h3>
    <p class="tour-body"></p>
    <div class="tour-footer">
      <span class="tour-progress"></span>
      <div class="tour-buttons">
        <button class="command-button tour-skip" type="button">Skip</button>
        <button class="command-button tour-back" type="button">Back</button>
        <button class="command-button is-primary tour-next" type="button">Next</button>
      </div>
    </div>`;
  card.querySelector(".tour-skip").addEventListener("click", endTour);
  card.querySelector(".tour-back").addEventListener("click", () => showStep(stepIndex - 1));
  card.querySelector(".tour-next").addEventListener("click", () => {
    if (stepIndex >= STEPS.length - 1) endTour();
    else showStep(stepIndex + 1);
  });

  layer.append(spotlight, card);
  document.body.append(layer);
}

function onKeyDown(event) {
  if (event.key === "Escape") {
    event.preventDefault();
    event.stopPropagation();
    endTour();
  } else if (event.key === "ArrowRight" || event.key === "Enter") {
    // preventDefault also stops the focused Next button from firing its own
    // click on Enter, which would advance twice.
    event.preventDefault();
    event.stopPropagation();
    if (stepIndex >= STEPS.length - 1) endTour();
    else showStep(stepIndex + 1);
  } else if (event.key === "ArrowLeft") {
    event.preventDefault();
    event.stopPropagation();
    showStep(stepIndex - 1);
  }
}

function showStep(index) {
  stepIndex = Math.max(0, Math.min(index, STEPS.length - 1));
  const step = STEPS[stepIndex];

  restoreDetails();
  const target = step.target ? document.querySelector(step.target) : null;
  if (target) {
    // Panel sections hold their content in a <details> child; module rows
    // ARE a <details> nested inside a collapsed panel. Open the target's own
    // details plus every <details> ancestor so the spotlight has something
    // to frame, and remember which ones we opened so they close again.
    const chain = new Set();
    const inner = target.matches("details") ? target : target.querySelector("details");
    if (inner) chain.add(inner);
    for (let d = target.closest("details"); d; d = d.parentElement?.closest("details")) {
      chain.add(d);
    }
    for (const details of chain) {
      if (!details.open) {
        details.open = true;
        openedDetails.push(details);
      }
    }
    target.scrollIntoView({ block: "nearest" });
  }

  card.querySelector(".tour-title").textContent = step.title;
  card.querySelector(".tour-body").textContent = step.body;
  card.querySelector(".tour-progress").textContent = `${stepIndex + 1} / ${STEPS.length}`;
  card.querySelector(".tour-back").disabled = stepIndex === 0;
  card.querySelector(".tour-next").textContent =
    stepIndex === STEPS.length - 1 ? "Start Bending" : "Next";
  card.querySelector(".tour-skip").hidden = stepIndex === STEPS.length - 1;

  reposition();
  card.querySelector(".tour-next").focus();
}

function restoreDetails() {
  for (const details of openedDetails) details.open = false;
  openedDetails = [];
}

function reposition() {
  if (!active) return;
  const step = STEPS[stepIndex];
  const target = step.target ? document.querySelector(step.target) : null;

  if (!target) {
    layer.classList.add("is-centered");
    card.style.left = `${Math.round((window.innerWidth - card.offsetWidth) / 2)}px`;
    card.style.top = `${Math.round((window.innerHeight - card.offsetHeight) / 2)}px`;
    return;
  }

  layer.classList.remove("is-centered");
  const pad = 6;
  const rect = target.getBoundingClientRect();
  spotlight.style.left = `${Math.round(rect.left - pad)}px`;
  spotlight.style.top = `${Math.round(rect.top - pad)}px`;
  spotlight.style.width = `${Math.round(rect.width + pad * 2)}px`;
  spotlight.style.height = `${Math.round(rect.height + pad * 2)}px`;

  // Place the card beside the highlighted section, on whichever side of the
  // screen has room (left panel -> card on the right, and vice versa). Topbar
  // targets get the card below them instead, centered on the highlight.
  const gap = 16;
  const cardWidth = card.offsetWidth;
  const cardHeight = card.offsetHeight;
  let left;
  let top;
  if (rect.bottom < window.innerHeight * 0.2) {
    left = rect.left + rect.width / 2 - cardWidth / 2;
    top = rect.bottom + pad + gap;
  } else {
    left =
      rect.left + rect.width / 2 < window.innerWidth / 2
        ? rect.right + pad + gap
        : rect.left - pad - gap - cardWidth;
    top = rect.top;
  }
  left = Math.max(12, Math.min(left, window.innerWidth - cardWidth - 12));
  top = Math.max(12, Math.min(top, window.innerHeight - cardHeight - 12));
  card.style.left = `${Math.round(left)}px`;
  card.style.top = `${Math.round(top)}px`;
}
