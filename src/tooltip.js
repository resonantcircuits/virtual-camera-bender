// Instant tooltips: reuses the title attributes already set throughout the
// app, but shows them in a themed bubble after a short delay instead of the
// browser's ~1s native hover. On first hover the title is moved to data-tip
// so the native tooltip stays suppressed; when the user turns tooltips off,
// the attribute is moved back so slow native titles remain as a fallback.

const STORAGE_KEY = "vcb.tooltips";
const SHOW_DELAY_MS = 150;

let enabled = true;
try {
  enabled = localStorage.getItem(STORAGE_KEY) !== "off";
} catch {
  // storage unavailable (private mode); default stays on
}

let bubble = null;
let showTimer = 0;
let currentTarget = null;

export function tooltipsEnabled() {
  return enabled;
}

export function setTooltipsEnabled(value) {
  enabled = Boolean(value);
  try {
    localStorage.setItem(STORAGE_KEY, enabled ? "on" : "off");
  } catch {
    // ignore
  }
  if (!enabled) hideTooltip();
}

export function initTooltips() {
  document.addEventListener("pointerover", handlePointerOver);
  document.addEventListener("pointerout", handlePointerOut);
  document.addEventListener("pointerdown", hideTooltip, true);
  document.addEventListener("scroll", hideTooltip, true);
  window.addEventListener("blur", hideTooltip);
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") hideTooltip();
  });
}

function handlePointerOver(event) {
  const target = event.target.closest?.("[title], [data-tip]");
  if (!target) return;

  if (!enabled) {
    // Restore native titles on elements we stripped earlier this session.
    if (!target.getAttribute("title") && target.dataset.tip) {
      target.setAttribute("title", target.dataset.tip);
      delete target.dataset.tip;
    }
    return;
  }

  // A fresh title always wins over a stale data-tip (code updates titles).
  const title = target.getAttribute("title");
  if (title) {
    target.dataset.tip = title;
    target.removeAttribute("title");
  }
  const text = target.dataset.tip;
  if (!text) return;
  if (target === currentTarget) return;

  currentTarget = target;
  clearTimeout(showTimer);
  showTimer = setTimeout(() => showTooltip(target, text), SHOW_DELAY_MS);
}

function handlePointerOut(event) {
  if (!currentTarget) return;
  const to = event.relatedTarget;
  if (to && currentTarget.contains(to)) return;
  if (currentTarget.contains(event.target)) hideTooltip();
}

function showTooltip(target, text) {
  if (!target.isConnected) return;
  if (!bubble) {
    bubble = document.createElement("div");
    bubble.className = "vcb-tooltip";
    bubble.setAttribute("role", "tooltip");
  }
  bubble.textContent = text;
  // Modal dialogs live in the top layer, so the bubble must be appended
  // inside the open dialog to render above it.
  const host = target.closest("dialog[open]") || document.body;
  if (bubble.parentElement !== host) host.append(bubble);

  const rect = target.getBoundingClientRect();
  bubble.style.left = "0px";
  bubble.style.top = "0px";
  const size = bubble.getBoundingClientRect();
  let left = rect.left + rect.width / 2 - size.width / 2;
  left = Math.max(8, Math.min(left, window.innerWidth - size.width - 8));
  let top = rect.bottom + 8;
  if (top + size.height > window.innerHeight - 8) top = rect.top - size.height - 8;
  bubble.style.left = `${Math.round(left)}px`;
  bubble.style.top = `${Math.round(top)}px`;
}

function hideTooltip() {
  clearTimeout(showTimer);
  showTimer = 0;
  currentTarget = null;
  bubble?.remove();
}
