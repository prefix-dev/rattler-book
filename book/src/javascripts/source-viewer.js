// ---------------------------------------------------------------------------
// Source Code Drawer: full-screen file browser with CodeMirror 6
// ---------------------------------------------------------------------------

var ICON_SOURCE = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M14.6 16.6l4.6-4.6-4.6-4.6L16 6l6 6-6 6-1.4-1.4m-5.2 0L4.8 12l4.6-4.6L8 6l-6 6 6 6 1.4-1.4Z"/></svg>';

// State
var manifest = null;
var cmView = null;
var cmModules = null;
var activeFilePath = null;
var _flatFilesCache = null;
var _triggerBtn = null;

// Safe localStorage wrapper
function storageSet(key, value) {
  try { localStorage.setItem(key, value); } catch (e) { /* private mode */ }
}

function closeDrawer() {
  document.body.classList.remove("source-drawer-open");
  storageSet("rb-source-viewer", "false");
  var btn = document.querySelector(".source-viewer-toggle");
  if (btn) {
    btn.setAttribute("aria-expanded", "false");
    btn.focus();
  }
}

// ---------------------------------------------------------------------------
// Toggle button in header
// ---------------------------------------------------------------------------
function initSourceViewer() {
  if (document.querySelector(".source-viewer-toggle")) return;

  var palette = document.querySelector("[data-md-component=palette]");
  if (!palette) return;

  var btn = document.createElement("button");
  btn.className = "source-viewer-toggle md-header__button md-icon";
  btn.title = "Browse source code";
  btn.setAttribute("aria-expanded", "false");
  btn.setAttribute("aria-label", "Browse source code");
  btn.innerHTML = ICON_SOURCE;
  btn.addEventListener("click", function () {
    toggleDrawer();
  });

  palette.parentNode.insertBefore(btn, palette);
  _triggerBtn = btn;
}

function toggleDrawer() {
  var isOpen = document.body.classList.contains("source-drawer-open");
  if (isOpen) {
    closeDrawer();
  } else {
    document.body.classList.add("source-drawer-open");
    ensureDrawer();
    storageSet("rb-source-viewer", "true");
    var btn = document.querySelector(".source-viewer-toggle");
    if (btn) btn.setAttribute("aria-expanded", "true");
    // Focus the close button for keyboard users
    var close = document.querySelector(".source-drawer__close");
    if (close) close.focus();
  }
}

// ---------------------------------------------------------------------------
// Drawer DOM
// ---------------------------------------------------------------------------
function ensureDrawer() {
  if (document.querySelector(".source-drawer")) {
    loadManifestIfNeeded();
    return;
  }

  // Backdrop
  var backdrop = document.createElement("div");
  backdrop.className = "source-drawer-backdrop";
  backdrop.setAttribute("aria-hidden", "true");
  document.body.appendChild(backdrop);

  backdrop.addEventListener("click", closeDrawer);

  // Drawer panel
  var drawer = document.createElement("div");
  drawer.className = "source-drawer";
  drawer.setAttribute("role", "dialog");
  drawer.setAttribute("aria-modal", "true");
  drawer.setAttribute("aria-label", "Source code viewer");
  drawer.innerHTML =
    '<div class="source-drawer__handle" aria-hidden="true"></div>' +
    '<div class="source-drawer__header">' +
      '<span class="source-drawer__title" id="source-drawer-title">Project Files</span>' +
      '<button class="source-drawer__close" aria-label="Close source viewer">&times;</button>' +
    '</div>' +
    '<nav class="source-drawer__tree" aria-label="File tree">' +
      '<div class="source-drawer__spinner" role="status">Loading...</div>' +
    '</nav>' +
    '<div class="source-drawer__code">' +
      '<div class="source-drawer__code-header">' +
        '<button class="source-drawer__back-btn" aria-label="Back to file tree">&larr; Back</button>' +
        '<span class="source-drawer__code-path"></span>' +
      '</div>' +
      '<div class="source-drawer__code-body">' +
        '<div class="source-drawer__placeholder">Select a file to view</div>' +
      '</div>' +
    '</div>';

  drawer.setAttribute("aria-labelledby", "source-drawer-title");
  document.body.appendChild(drawer);

  // Close button
  drawer.querySelector(".source-drawer__close").addEventListener("click", closeDrawer);

  // Back button (mobile)
  drawer.querySelector(".source-drawer__back-btn").addEventListener("click", function () {
    drawer.classList.remove("viewing-file");
    activeFilePath = null;
    updateActiveFileBtn();
  });

  // Drag handle to dismiss
  var handle = drawer.querySelector(".source-drawer__handle");
  var dragStartY = 0;
  var dragging = false;

  function onDragStart(e) {
    dragging = true;
    dragStartY = (e.touches ? e.touches[0] : e).clientY;
    drawer.style.transition = "none";
  }

  function onDragMove(e) {
    if (!dragging) return;
    if (e.cancelable) e.preventDefault();
    var clientY = (e.touches ? e.touches[0] : e).clientY;
    var dy = Math.max(0, clientY - dragStartY);
    drawer.style.transform = "translateY(" + dy + "px)";
  }

  function onDragEnd(e) {
    if (!dragging) return;
    dragging = false;
    drawer.style.transition = "";
    var touch = e.changedTouches ? e.changedTouches[0] : e;
    if (!touch) { drawer.style.transform = ""; return; }
    var dy = touch.clientY - dragStartY;
    if (dy > 100) {
      closeDrawer();
    }
    drawer.style.transform = "";
  }

  handle.addEventListener("mousedown", onDragStart);
  document.addEventListener("mousemove", onDragMove);
  document.addEventListener("mouseup", onDragEnd);
  handle.addEventListener("touchstart", onDragStart, { passive: true });
  document.addEventListener("touchmove", onDragMove, { passive: false });
  document.addEventListener("touchend", onDragEnd);

  // Escape key
  document.addEventListener("keydown", function (e) {
    if (e.key === "Escape" && document.body.classList.contains("source-drawer-open")) {
      closeDrawer();
    }
  });

  loadManifestIfNeeded();
}

// ---------------------------------------------------------------------------
// Manifest loading
// ---------------------------------------------------------------------------
function loadManifestIfNeeded() {
  if (manifest) return;

  // Derive manifest URL from this script's own src (works on any host/path)
  var manifestUrl = "./source-manifest.json";
  var scripts = document.querySelectorAll("script[src*='source-viewer']");
  if (scripts.length) {
    manifestUrl = scripts[0].src.replace(/javascripts\/source-viewer\.js.*/, "source-manifest.json");
  }

  fetch(manifestUrl)
    .then(function (r) {
      if (!r.ok) throw new Error("HTTP " + r.status);
      return r.json();
    })
    .then(function (data) {
      manifest = data;
      _flatFilesCache = null;
      renderTree();
    })
    .catch(function (err) {
      console.error("Failed to load source manifest:", err);
      var tree = document.querySelector(".source-drawer__tree");
      if (tree) tree.innerHTML = '<div class="source-drawer__spinner">Failed to load files</div>';
    });
}

// ---------------------------------------------------------------------------
// File tree rendering
// ---------------------------------------------------------------------------
function renderTree() {
  var container = document.querySelector(".source-drawer__tree");
  if (!container || !manifest) return;

  container.innerHTML = "";
  var ul = document.createElement("ul");
  ul.setAttribute("role", "tree");
  renderTreeNodes(ul, manifest.tree, 1);
  container.appendChild(ul);
}

function renderTreeNodes(parentUl, nodes, level) {
  for (var i = 0; i < nodes.length; i++) {
    var node = nodes[i];
    var li = document.createElement("li");
    li.setAttribute("role", "treeitem");

    if (node.type === "dir") {
      var toggle = document.createElement("button");
      toggle.className = "source-drawer__dir-toggle open";
      toggle.setAttribute("aria-expanded", "true");
      toggle.innerHTML = '<span class="chevron" aria-hidden="true">&#9654;</span><span class="source-drawer__dir-icon" aria-hidden="true"></span> ' + escapeHtml(node.name);

      var childrenDiv = document.createElement("div");
      childrenDiv.className = "source-drawer__dir-children";
      var childUl = document.createElement("ul");
      childUl.setAttribute("role", "group");
      renderTreeNodes(childUl, node.children, level + 1);
      childrenDiv.appendChild(childUl);

      toggle.addEventListener("click", (function (t, c) {
        return function () {
          var isOpen = t.classList.toggle("open");
          c.classList.toggle("collapsed");
          t.setAttribute("aria-expanded", isOpen ? "true" : "false");
        };
      })(toggle, childrenDiv));

      li.appendChild(toggle);
      li.appendChild(childrenDiv);
    } else {
      var fileBtn = document.createElement("button");
      fileBtn.className = "source-drawer__file-btn";
      fileBtn.dataset.path = node.path;
      fileBtn.setAttribute("aria-label", node.path);
      fileBtn.innerHTML =
        '<span class="source-drawer__file-icon" aria-hidden="true"></span>' +
        escapeHtml(node.name);
      fileBtn.addEventListener("click", (function (path) {
        return function () { openFile(path); };
      })(node.path));
      li.appendChild(fileBtn);
    }

    parentUl.appendChild(li);
  }
}

function escapeHtml(str) {
  var div = document.createElement("div");
  div.appendChild(document.createTextNode(str));
  return div.innerHTML;
}

// ---------------------------------------------------------------------------
// File opening + CodeMirror
// ---------------------------------------------------------------------------
function openFile(path) {
  if (!manifest || !manifest.files[path]) return;

  activeFilePath = path;
  updateActiveFileBtn();

  // Mobile: switch to file view
  var drawer = document.querySelector(".source-drawer");
  if (drawer) drawer.classList.add("viewing-file");

  // Update path display
  var pathEl = document.querySelector(".source-drawer__code-path");
  if (pathEl) pathEl.textContent = path;

  var body = document.querySelector(".source-drawer__code-body");
  if (!body) return;

  var content = manifest.files[path];
  var lang = getLangForPath(path);

  if (_cmFailed) {
    // CDN previously failed; use plain text
    createFallbackView(body, content);
  } else if (!cmModules) {
    body.innerHTML = '<div class="source-drawer__spinner" role="status">Loading editor...</div>';
    loadCodeMirror()
      .then(function () {
        if (!document.body.classList.contains("source-drawer-open")) return;
        createEditor(body, content, lang);
      })
      .catch(function (err) {
        console.warn("CodeMirror unavailable (offline?), using plain text fallback:", err);
        createFallbackView(body, content);
      });
  } else {
    createEditor(body, content, lang);
  }
}

function getLangForPath(path) {
  var files = flatFiles(manifest.tree);
  for (var i = 0; i < files.length; i++) {
    if (files[i].path === path) return files[i].lang;
  }
  return "text";
}

function updateActiveFileBtn() {
  var btns = document.querySelectorAll(".source-drawer__file-btn");
  for (var i = 0; i < btns.length; i++) {
    var isActive = btns[i].dataset.path === activeFilePath;
    btns[i].classList.toggle("active", isActive);
    btns[i].setAttribute("aria-current", isActive ? "true" : "false");
  }
}

function flatFiles(tree) {
  if (_flatFilesCache) return _flatFilesCache;
  var result = [];
  function walk(nodes) {
    for (var i = 0; i < nodes.length; i++) {
      if (nodes[i].type === "file") result.push(nodes[i]);
      else if (nodes[i].children) walk(nodes[i].children);
    }
  }
  walk(tree);
  _flatFilesCache = result;
  return result;
}

// ---------------------------------------------------------------------------
// CodeMirror 6 dynamic loading from CDN
// ---------------------------------------------------------------------------
// Load without ?bundle so esm.sh resolves shared deps (like @codemirror/state)
// to the same URL, avoiding duplicate instance issues.

function loadCodeMirror() {
  if (cmModules) return Promise.resolve();

  return Promise.all([
    import("https://esm.sh/@codemirror/view@6"),
    import("https://esm.sh/@codemirror/state@6"),
    import("https://esm.sh/@codemirror/language@6"),
    import("https://esm.sh/@codemirror/commands@6"),
    import("https://esm.sh/@codemirror/search@6"),
    import("https://esm.sh/@codemirror/autocomplete@6"),
    import("https://esm.sh/@codemirror/lint@6"),
    import("https://esm.sh/@codemirror/lang-rust@6"),
    import("https://esm.sh/@codemirror/legacy-modes@6/mode/lua"),
    import("https://esm.sh/@lezer/highlight@1"),
  ]).then(function (modules) {
    var view = modules[0];
    var state = modules[1];
    var language = modules[2];
    var commands = modules[3];
    var search = modules[4];
    var autocomplete = modules[5];
    var lint = modules[6];
    var langRust = modules[7];
    var luaMode = modules[8];
    var highlight = modules[9];
    var t = highlight.tags;

    // Gruvbox Light highlight style (matches book's Pygments theme)
    var gruvboxLight = language.HighlightStyle.define([
      { tag: [t.comment, t.lineComment, t.blockComment], color: "#928374", fontStyle: "italic" },
      { tag: [t.keyword, t.definitionKeyword, t.modifier, t.operatorKeyword], color: "#9d0006" },
      { tag: [t.typeName, t.typeOperator], color: "#b57614" },
      { tag: [t.string, t.special(t.string), t.character], color: "#79740e" },
      { tag: [t.number, t.integer, t.float], color: "#8f3f71" },
      { tag: [t.operator, t.compareOperator, t.updateOperator], color: "#af3a03" },
      { tag: [t.function(t.definition(t.variableName)), t.function(t.variableName)], color: "#427b58", fontWeight: "bold" },
      { tag: [t.definition(t.typeName), t.className], color: "#b57614", fontWeight: "bold" },
      { tag: [t.variableName], color: "#076678" },
      { tag: [t.self, t.special(t.variableName)], color: "#076678" },
      { tag: [t.standard(t.name), t.bool], color: "#af3a03" },
      { tag: [t.attributeName, t.macroName], color: "#79740e" },
      { tag: [t.punctuation, t.separator, t.bracket], color: "#3c3836" },
      { tag: [t.name], color: "#3c3836" },
      { tag: [t.meta, t.processingInstruction], color: "#427b58" },
      { tag: [t.labelName], color: "#427b58" },
      { tag: [t.constant(t.name)], color: "#8f3f71" },
      { tag: [t.escape], color: "#af3a03" },
      { tag: [t.invalid], color: "#9d0006", backgroundColor: "#f9e0c7" },
    ]);

    // Gruvbox Dark highlight style
    var gruvboxDark = language.HighlightStyle.define([
      { tag: [t.comment, t.lineComment, t.blockComment], color: "#928374", fontStyle: "italic" },
      { tag: [t.keyword, t.definitionKeyword, t.modifier, t.operatorKeyword], color: "#fb4934" },
      { tag: [t.typeName, t.typeOperator], color: "#fabd2f" },
      { tag: [t.string, t.special(t.string), t.character], color: "#b8bb26" },
      { tag: [t.number, t.integer, t.float], color: "#d3869b" },
      { tag: [t.operator, t.compareOperator, t.updateOperator], color: "#fe8019" },
      { tag: [t.function(t.definition(t.variableName)), t.function(t.variableName)], color: "#8ec07c", fontWeight: "bold" },
      { tag: [t.definition(t.typeName), t.className], color: "#fabd2f", fontWeight: "bold" },
      { tag: [t.variableName], color: "#83a598" },
      { tag: [t.self, t.special(t.variableName)], color: "#83a598" },
      { tag: [t.standard(t.name), t.bool], color: "#fe8019" },
      { tag: [t.attributeName, t.macroName], color: "#b8bb26" },
      { tag: [t.punctuation, t.separator, t.bracket], color: "#ebdbb2" },
      { tag: [t.name], color: "#ebdbb2" },
      { tag: [t.meta, t.processingInstruction], color: "#8ec07c" },
      { tag: [t.labelName], color: "#8ec07c" },
      { tag: [t.constant(t.name)], color: "#d3869b" },
      { tag: [t.escape], color: "#fe8019" },
      { tag: [t.invalid], color: "#fb4934", backgroundColor: "#3c1f1e" },
    ]);

    // Pick highlight style based on current theme
    function getHighlightStyle() {
      var scheme = document.documentElement.getAttribute("data-md-color-scheme");
      return scheme === "slate" ? gruvboxDark : gruvboxLight;
    }

    // Assemble basicSetup equivalent (same as codemirror's basicSetup export)
    var basicSetup = [
      view.lineNumbers(),
      view.highlightActiveLineGutter(),
      view.highlightSpecialChars(),
      commands.history(),
      language.foldGutter(),
      view.drawSelection(),
      view.dropCursor(),
      state.EditorState.allowMultipleSelections.of(true),
      language.indentOnInput(),
      language.bracketMatching(),
      autocomplete.closeBrackets(),
      view.rectangularSelection(),
      view.crosshairCursor(),
      view.highlightActiveLine(),
      search.highlightSelectionMatches(),
      view.keymap.of([
        ...autocomplete.closeBracketsKeymap,
        ...commands.defaultKeymap,
        ...search.searchKeymap,
        ...commands.historyKeymap,
        ...language.foldKeymap,
        ...autocomplete.completionKeymap,
        ...lint.lintKeymap,
      ]),
    ];

    cmModules = {
      basicSetup: basicSetup,
      langRust: langRust,
      langLua: language.StreamLanguage.define(luaMode.lua),
      view: view,
      state: state,
      language: language,
      getHighlightStyle: getHighlightStyle,
    };
  });
}

function createEditor(container, content, lang) {
  container.innerHTML = "";

  var extensions = [
    cmModules.basicSetup,
    cmModules.state.EditorState.readOnly.of(true),
    cmModules.language.syntaxHighlighting(cmModules.getHighlightStyle()),
    cmModules.view.EditorView.theme({
      "&": {
        height: "100%",
        backgroundColor: "var(--rb-code-bg)",
        color: "var(--rb-code-text)",
      },
      ".cm-gutters": {
        backgroundColor: "var(--rb-code-bg)",
        color: "var(--rb-text-lighter)",
        border: "none",
        borderRight: "1px solid var(--rb-code-border)",
      },
      ".cm-activeLineGutter": {
        backgroundColor: "transparent",
      },
      ".cm-activeLine": {
        backgroundColor: "transparent",
      },
      ".cm-cursor": {
        display: "none",
      },
    }),
  ];

  // Add language support
  if (lang === "rust" && cmModules.langRust && cmModules.langRust.rust) {
    extensions.push(cmModules.langRust.rust());
  } else if (lang === "lua" && cmModules.langLua) {
    extensions.push(cmModules.langLua);
  }

  var state = cmModules.state.EditorState.create({
    doc: content,
    extensions: extensions,
  });

  if (cmView) {
    cmView.destroy();
    cmView = null;
  }

  cmView = new cmModules.view.EditorView({
    state: state,
    parent: container,
  });
}

// Plain-text fallback when CodeMirror is unavailable (offline, CDN blocked)
var _cmFailed = false;

function createFallbackView(container, content) {
  _cmFailed = true;
  container.innerHTML = "";
  var pre = document.createElement("pre");
  pre.style.cssText =
    "margin:0; padding:1em 1.2em; overflow:auto; height:100%;" +
    "font-family:var(--rb-font-code); font-size:calc(var(--rb-body-size)*0.85);" +
    "line-height:1.5; color:var(--rb-code-text); background:var(--rb-code-bg);" +
    "white-space:pre; tab-size:4;";
  var code = document.createElement("code");
  code.textContent = content;
  pre.appendChild(code);
  container.appendChild(pre);
}

// ---------------------------------------------------------------------------
// Init + SPA support
// ---------------------------------------------------------------------------
initSourceViewer();

if (typeof document$ !== "undefined") {
  document$.subscribe(function () {
    initSourceViewer();
  });
}

document.addEventListener("DOMContentLoaded", function () {
  initSourceViewer();
});
