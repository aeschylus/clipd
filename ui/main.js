// clipd panel frontend
//
// Communicates with the Tauri backend via window.__TAURI__.core.invoke().
// All state is kept in a single plain object (functional-style: we replace
// state rather than mutating individual fields).

const invoke = window.__TAURI__?.core?.invoke ?? (() => Promise.resolve([]));
const listen  = window.__TAURI__?.event?.listen;

// ─── State ────────────────────────────────────────────────────────────────────

let state = {
  clips: [],         // ClipView[]
  filtered: [],      // ClipView[] after search filter
  selectedIndex: 0,  // currently highlighted item index
  query: "",         // current search string
  loading: false,
};

const setState = (patch) => {
  state = { ...state, ...patch };
};

// ─── DOM refs ─────────────────────────────────────────────────────────────────

const searchInput = document.getElementById("search-input");
const clipList    = document.getElementById("clip-list");
const emptyState  = document.getElementById("empty-state");
const emptyMsg    = document.getElementById("empty-message");
const clipCount   = document.getElementById("clip-count");
const escHint     = document.getElementById("esc-hint");

// ─── Rendering ───────────────────────────────────────────────────────────────

/**
 * Build the icon SVG markup for a given type identifier.
 */
const typeIconSvg = (typeIcon) => {
  const icons = {
    link: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71"/><path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71"/></svg>`,
    code: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><polyline points="16 18 22 12 16 6"/><polyline points="8 6 2 12 8 18"/></svg>`,
    folder: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>`,
    image: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="8.5" cy="8.5" r="1.5"/><polyline points="21 15 16 10 5 21"/></svg>`,
    text: `<svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><line x1="17" y1="10" x2="3" y2="10"/><line x1="21" y1="6" x2="3" y2="6"/><line x1="21" y1="14" x2="3" y2="14"/><line x1="17" y1="18" x2="3" y2="18"/></svg>`,
  };
  return icons[typeIcon] ?? icons.text;
};

/**
 * Highlight occurrences of `query` in `text`.
 * Returns HTML string with <mark> tags around matches.
 */
const highlight = (text, query) => {
  if (!query) return escapeHtml(text);
  const re = new RegExp(`(${escapeRegex(query)})`, "gi");
  return escapeHtml(text).replace(re, "<mark>$1</mark>");
};

const escapeHtml = (s) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

const escapeRegex = (s) =>
  s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * Render a single ClipView as an HTML element.
 */
const renderClipItem = (clip, index, query) => {
  const isSelected = index === state.selectedIndex;
  const el = document.createElement("div");
  el.className = [
    "clip-item",
    `type-${clip.type_icon}`,
    isSelected ? "selected" : "",
    clip.pinned ? "pinned" : "",
  ]
    .filter(Boolean)
    .join(" ");
  el.setAttribute("role", "option");
  el.setAttribute("aria-selected", String(isSelected));
  el.dataset.index = String(index);
  el.dataset.id = String(clip.id);

  const metaParts = [
    clip.source_app ? `<span class="clip-app">${escapeHtml(clip.source_app)}</span>` : "",
    clip.source_app ? `<span class="clip-separator"></span>` : "",
    `<span>${escapeHtml(clip.created_at)}</span>`,
    clip.label ? `<span class="clip-separator"></span><span class="clip-label">${escapeHtml(clip.label)}</span>` : "",
  ].join("");

  el.innerHTML = `
    <div class="clip-type-icon type-${clip.type_icon}">${typeIconSvg(clip.type_icon)}</div>
    <div class="clip-content">
      <div class="clip-preview">${highlight(clip.preview, query)}</div>
      <div class="clip-meta">${metaParts}</div>
    </div>
    <button class="clip-delete" title="Delete (⌘⌫)" aria-label="Delete clip" tabindex="-1">×</button>
  `;

  // Click on item body: paste
  el.addEventListener("click", (e) => {
    if (e.target.closest(".clip-delete")) return;
    pasteClip(clip.id);
  });

  // Click delete button
  el.querySelector(".clip-delete").addEventListener("click", (e) => {
    e.stopPropagation();
    deleteClip(clip.id, index);
  });

  return el;
};

/**
 * Full re-render of the clip list from current state.
 */
const renderList = () => {
  const { filtered, selectedIndex, query } = state;

  // Remove all existing clip-item elements (keep empty-state)
  Array.from(clipList.querySelectorAll(".clip-item")).forEach((el) => el.remove());

  if (filtered.length === 0) {
    emptyState.classList.remove("hidden");
    emptyMsg.textContent = query ? `No clips matching "${query}"` : "No clips yet.";
    clipCount.textContent = "0 clips";
    return;
  }

  emptyState.classList.add("hidden");
  clipCount.textContent = `${filtered.length} clip${filtered.length !== 1 ? "s" : ""}`;

  const frag = document.createDocumentFragment();
  filtered.forEach((clip, i) => {
    frag.appendChild(renderClipItem(clip, i, query));
  });
  clipList.appendChild(frag);

  // Ensure selected item is visible
  scrollToSelected();
};

const scrollToSelected = () => {
  const selected = clipList.querySelector(".clip-item.selected");
  if (selected) {
    selected.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }
};

// ─── Data loading ─────────────────────────────────────────────────────────────

const loadClips = async () => {
  if (state.loading) return;
  setState({ loading: true });
  try {
    const clips = await invoke("list_clips", { limit: 100 });
    const filtered = applyFilter(clips, state.query);
    setState({ clips, filtered, selectedIndex: 0, loading: false });
    renderList();
  } catch (err) {
    console.error("list_clips error:", err);
    setState({ loading: false });
  }
};

const searchClips = async (query) => {
  setState({ query });
  if (!query.trim()) {
    const filtered = applyFilter(state.clips, "");
    setState({ filtered, selectedIndex: 0 });
    renderList();
    return;
  }
  try {
    const clips = await invoke("search_clips", { query, limit: 100 });
    const filtered = applyFilter(clips, query);
    setState({ filtered, selectedIndex: 0 });
    renderList();
  } catch (err) {
    console.error("search_clips error:", err);
  }
};

/**
 * Client-side filter as a fast path before the FTS query.
 * For empty queries, just returns the full list.
 */
const applyFilter = (clips, query) => {
  if (!query.trim()) return clips;
  const q = query.toLowerCase();
  return clips.filter(
    (c) =>
      c.preview.toLowerCase().includes(q) ||
      (c.source_app ?? "").toLowerCase().includes(q) ||
      (c.label ?? "").toLowerCase().includes(q) ||
      c.tags.some((t) => t.toLowerCase().includes(q))
  );
};

// ─── Actions ──────────────────────────────────────────────────────────────────

const pasteClip = async (id) => {
  try {
    await invoke("paste_clip", { id });
    // Window will be hidden by the backend; reset state for next open
    resetPanel();
  } catch (err) {
    console.error("paste_clip error:", err);
  }
};

const deleteClip = async (id, index) => {
  try {
    await invoke("delete_clip", { id });
    // Remove from local state without a full reload
    const newClips    = state.clips.filter((c) => c.id !== id);
    const newFiltered = state.filtered.filter((c) => c.id !== id);
    const newIndex    = Math.min(index, newFiltered.length - 1);
    setState({ clips: newClips, filtered: newFiltered, selectedIndex: Math.max(0, newIndex) });
    renderList();
  } catch (err) {
    console.error("delete_clip error:", err);
  }
};

const resetPanel = () => {
  searchInput.value = "";
  setState({ query: "", selectedIndex: 0 });
};

// ─── Keyboard navigation ──────────────────────────────────────────────────────

const moveSelection = (delta) => {
  const n = state.filtered.length;
  if (n === 0) return;
  const next = Math.max(0, Math.min(n - 1, state.selectedIndex + delta));
  setState({ selectedIndex: next });

  // Update DOM without full re-render for smoothness
  clipList.querySelectorAll(".clip-item").forEach((el, i) => {
    const sel = i === next;
    el.classList.toggle("selected", sel);
    el.setAttribute("aria-selected", String(sel));
  });
  scrollToSelected();
};

document.addEventListener("keydown", async (e) => {
  switch (e.key) {
    case "Escape":
      // Hide window via Tauri
      try { await window.__TAURI__?.window?.getCurrent()?.hide(); } catch {}
      resetPanel();
      break;

    case "ArrowDown":
      e.preventDefault();
      moveSelection(1);
      break;

    case "ArrowUp":
      e.preventDefault();
      moveSelection(-1);
      break;

    case "Enter": {
      e.preventDefault();
      const clip = state.filtered[state.selectedIndex];
      if (clip) pasteClip(clip.id);
      break;
    }

    case "Backspace":
      if (e.metaKey || e.ctrlKey) {
        e.preventDefault();
        const clip = state.filtered[state.selectedIndex];
        if (clip) deleteClip(clip.id, state.selectedIndex);
      }
      break;

    default:
      // Any printable key redirects focus to search input
      if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && document.activeElement !== searchInput) {
        searchInput.focus();
      }
  }
});

// ─── Search input ─────────────────────────────────────────────────────────────

let searchDebounceTimer = null;

searchInput.addEventListener("input", () => {
  const query = searchInput.value;
  escHint.classList.toggle("visible", query.length > 0);

  clearTimeout(searchDebounceTimer);
  searchDebounceTimer = setTimeout(() => {
    searchClips(query);
  }, 120);
});

searchInput.addEventListener("keydown", (e) => {
  // Prevent arrow keys from moving input cursor while navigating list
  if (e.key === "ArrowUp" || e.key === "ArrowDown") {
    e.preventDefault();
  }
});

// ─── Tauri event listeners ────────────────────────────────────────────────────

// Backend emits "refresh-clips" when the panel is shown
if (listen) {
  listen("refresh-clips", () => {
    searchInput.value = "";
    setState({ query: "" });
    loadClips();
    searchInput.focus();
  });
}

// ─── Init ─────────────────────────────────────────────────────────────────────

// Focus search on load
searchInput.focus();

// Initial data load
loadClips();
