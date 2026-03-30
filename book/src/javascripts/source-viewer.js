// ---------------------------------------------------------------------------
// Source Code Drawer: full-screen file browser with CodeMirror 6
// ---------------------------------------------------------------------------

var ICON_SOURCE = '&lt;/&gt;';

// State
var manifest = null;
var cmView = null;
var cmModules = null;
var activeFilePath = null;
var _flatFilesCache = null;
var _triggerBtn = null;
var _highlightEffect = null;

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
  btn.className = "source-viewer-toggle md-header__button";
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
    '<div class="source-drawer__resize-handle" aria-hidden="true"></div>' +
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

  // Resizable tree/code split
  var resizeHandle = drawer.querySelector(".source-drawer__resize-handle");
  var resizing = false;

  function onResizeStart(e) {
    resizing = true;
    resizeHandle.classList.add("dragging");
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
    if (e.cancelable) e.preventDefault();
  }

  function onResizeMove(e) {
    if (!resizing) return;
    if (e.cancelable) e.preventDefault();
    var clientX = (e.touches ? e.touches[0] : e).clientX;
    var drawerRect = drawer.getBoundingClientRect();
    var newWidth = Math.max(180, Math.min(clientX - drawerRect.left, drawerRect.width * 0.6));
    drawer.style.setProperty("--source-tree-width", newWidth + "px");
    storageSet("rb-tree-width", Math.round(newWidth));
  }

  function onResizeEnd() {
    if (!resizing) return;
    resizing = false;
    resizeHandle.classList.remove("dragging");
    document.body.style.cursor = "";
    document.body.style.userSelect = "";
  }

  resizeHandle.addEventListener("mousedown", onResizeStart);
  document.addEventListener("mousemove", onResizeMove);
  document.addEventListener("mouseup", onResizeEnd);
  resizeHandle.addEventListener("touchstart", onResizeStart, { passive: false });
  document.addEventListener("touchmove", onResizeMove, { passive: false });
  document.addEventListener("touchend", onResizeEnd);

  // Restore saved tree width
  try {
    var savedWidth = localStorage.getItem("rb-tree-width");
    if (savedWidth) drawer.style.setProperty("--source-tree-width", savedWidth + "px");
  } catch (e) { /* private mode */ }

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
var _manifestPromise = null;

function loadManifestIfNeeded() {
  if (manifest) return Promise.resolve();
  if (_manifestPromise) return _manifestPromise;

  // Derive manifest URL from this script's own src (works on any host/path)
  var manifestUrl = "./source-manifest.json";
  var scripts = document.querySelectorAll("script[src*='source-viewer']");
  if (scripts.length) {
    manifestUrl = scripts[0].src.replace(/javascripts\/source-viewer\.js.*/, "source-manifest.json");
  }

  _manifestPromise = fetch(manifestUrl)
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
      _manifestPromise = null; // allow retry
      var tree = document.querySelector(".source-drawer__tree");
      if (tree) tree.innerHTML = '<div class="source-drawer__spinner">Failed to load files</div>';
    });

  return _manifestPromise;
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

  // Fold legend
  var legend = document.createElement("div");
  legend.className = "source-drawer__legend";
  legend.innerHTML =
    '<span class="source-drawer__legend-item"><span class="source-drawer__legend-icon legend-entangled">&raquo;</span> literate block</span>' +
    '<span class="source-drawer__legend-item"><span class="source-drawer__legend-icon legend-code">&#9656;</span> code fold</span>';
  container.appendChild(legend);
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
function openFile(path, blockId, markerSuffix) {
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
        if (blockId) scrollToBlock(blockId, markerSuffix);
      })
      .catch(function (err) {
        console.warn("CodeMirror unavailable (offline?), using plain text fallback:", err);
        createFallbackView(body, content);
      });
  } else {
    createEditor(body, content, lang);
    if (blockId) scrollToBlock(blockId, markerSuffix);
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

// ---------------------------------------------------------------------------
// Block navigation helpers
// ---------------------------------------------------------------------------

// Find which file in the manifest contains a given block ID
function findFileForBlock(blockId) {
  if (!manifest || !manifest.files) return null;
  var needle = "#" + blockId + ">>";
  var paths = Object.keys(manifest.files);
  for (var i = 0; i < paths.length; i++) {
    if (manifest.files[paths[i]].indexOf(needle) !== -1) return paths[i];
  }
  return null;
}

// Find the line range for a block in the current editor
function findBlockRange(blockId, markerSuffix) {
  if (!cmView) return null;
  var doc = cmView.state.doc;
  var needle = "#" + blockId + ">>[" + (markerSuffix || "init") + "]";
  var startLine = -1;

  for (var i = 1; i <= doc.lines; i++) {
    if (doc.line(i).text.indexOf(needle) !== -1) {
      startLine = i;
      break;
    }
  }
  if (startLine === -1) return null;

  // Find matching end, respecting nesting
  var depth = 1;
  var endLine = startLine;
  for (var j = startLine + 1; j <= doc.lines; j++) {
    var text = doc.line(j).text;
    if (text.indexOf("~/~ begin") !== -1) depth++;
    else if (text.indexOf("~/~ end") !== -1) {
      depth--;
      if (depth === 0) { endLine = j; break; }
    }
  }
  return { start: startLine, end: endLine };
}

// Scroll to and highlight a block in the current editor
function scrollToBlock(blockId, markerSuffix) {
  if (!cmView || !_highlightEffect || !cmModules) return;
  var range = findBlockRange(blockId, markerSuffix);
  if (!range) return;

  var doc = cmView.state.doc;
  var deco = cmModules.blockHighlightDeco;
  var builder = new cmModules.state.RangeSetBuilder();
  for (var line = range.start; line <= range.end; line++) {
    builder.add(doc.line(line).from, doc.line(line).from, deco);
  }

  cmView.dispatch({
    effects: [
      _highlightEffect.of(builder.finish()),
      cmModules.view.EditorView.scrollIntoView(
        doc.line(range.start).from,
        { y: "start", yMargin: 50 }
      ),
    ],
  });

  // Fade out highlight after 3 seconds
  setTimeout(function () {
    if (cmView) {
      cmView.dispatch({
        effects: _highlightEffect.of(cmModules.view.Decoration.none),
      });
    }
  }, 3000);
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

    // Entangled marker line decoration (dimmed)
    var markerLineDeco = view.Decoration.line({ class: "cm-entangled-marker" });
    var markerField = state.StateField.define({
      create: function (editorState) {
        var builder = new state.RangeSetBuilder();
        for (var i = 1; i <= editorState.doc.lines; i++) {
          if (/^\s*(\/\/|#|--)\s*~\/~/.test(editorState.doc.line(i).text)) {
            builder.add(editorState.doc.line(i).from, editorState.doc.line(i).from, markerLineDeco);
          }
        }
        return builder.finish();
      },
      update: function (decos) { return decos; },
      provide: function (f) { return view.EditorView.decorations.from(f); },
    });

    // Gutter class for entangled fold markers (distinct icon via CSS)
    class EntangledFoldMark extends view.GutterMarker {
      get elementClass() { return "cm-entangled-fold"; }
    }
    var entangledFoldMark = new EntangledFoldMark();
    var entangledGutterField = state.StateField.define({
      create: function (editorState) {
        var builder = new state.RangeSetBuilder();
        for (var i = 1; i <= editorState.doc.lines; i++) {
          if (/^\s*(\/\/|#|--)\s*~\/~\s*begin/.test(editorState.doc.line(i).text)) {
            builder.add(editorState.doc.line(i).from, editorState.doc.line(i).from, entangledFoldMark);
          }
        }
        return builder.finish();
      },
      update: function (marks) { return marks; },
      provide: function (f) { return view.gutterLineClass.from(f); },
    });

    // Block highlight decoration (driven by effect)
    _highlightEffect = state.StateEffect.define();
    var blockHighlightDeco = view.Decoration.line({ class: "cm-block-highlight" });
    var highlightField = state.StateField.define({
      create: function () { return view.Decoration.none; },
      update: function (decos, tr) {
        for (var i = 0; i < tr.effects.length; i++) {
          if (tr.effects[i].is(_highlightEffect)) return tr.effects[i].value;
        }
        return decos;
      },
      provide: function (f) { return view.EditorView.decorations.from(f); },
    });

    cmModules = {
      basicSetup: basicSetup,
      langRust: langRust,
      langLua: language.StreamLanguage.define(luaMode.lua),
      view: view,
      state: state,
      language: language,
      getHighlightStyle: getHighlightStyle,
      markerField: markerField,
      highlightField: highlightField,
      entangledGutterField: entangledGutterField,
      blockHighlightDeco: blockHighlightDeco,
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

  // Entangled marker + block highlight decorations
  if (cmModules.markerField) extensions.push(cmModules.markerField);
  if (cmModules.highlightField) extensions.push(cmModules.highlightField);
  if (cmModules.entangledGutterField) extensions.push(cmModules.entangledGutterField);

  // Fold service: begin/end marker blocks are collapsible via the gutter
  extensions.push(cmModules.language.foldService.of(function (state, lineStart) {
    var line = state.doc.lineAt(lineStart);
    if (line.text.indexOf("~/~ begin") === -1) return null;
    var depth = 1;
    for (var i = line.number + 1; i <= state.doc.lines; i++) {
      var text = state.doc.line(i).text;
      if (text.indexOf("~/~ begin") !== -1) depth++;
      else if (text.indexOf("~/~ end") !== -1) {
        depth--;
        if (depth === 0) {
          return { from: line.to, to: state.doc.line(i).to };
        }
      }
    }
    return null;
  }));

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
// File link buttons on entangled code blocks
// ---------------------------------------------------------------------------
function injectFileLinks() {
  var spans = document.querySelectorAll("span.filename");
  for (var i = 0; i < spans.length; i++) {
    var span = spans[i];
    if (span.querySelector(".source-link-btn")) continue;
    var text = span.textContent.trim();

    var path = null;
    var blockId = null;
    var markerSuffix = "init";

    // Extract file path (e.g. "file: src/main.rs")
    var fileMatch = text.match(/file:\s*(.+?)$/);
    if (fileMatch) path = fileMatch[1].trim();

    // Extract block ID (e.g. "#main-imports")
    var idMatch = text.match(/^#([^\s\[]+)/);
    if (idMatch) blockId = idMatch[1];

    // Extract index for multi-part blocks (e.g. "[1]", "[2]")
    // The plugin uses 1-based display: [1]=first, [2]=second, ...
    // Entangled markers use: [init]=first, [1]=second, [2]=third, ...
    var indexMatch = text.match(/\[(\d+)\]/);
    if (indexMatch) {
      var n = parseInt(indexMatch[1]);
      markerSuffix = n <= 1 ? "init" : String(n - 1);
    }

    if (!path && !blockId) continue;

    var btn = document.createElement("button");
    btn.className = "source-link-btn";
    if (path) btn.dataset.path = path;
    if (blockId) btn.dataset.blockId = blockId;
    btn.dataset.markerSuffix = markerSuffix;
    btn.title = "View in source";
    btn.setAttribute("aria-label", "View " + (path || "#" + blockId) + " in source viewer");
    btn.textContent = "</>";
    span.appendChild(btn);
  }
}

// Event delegation for file link buttons (registered once)
var _fileLinkDelegateRegistered = false;
function registerFileLinkDelegate() {
  if (_fileLinkDelegateRegistered) return;
  _fileLinkDelegateRegistered = true;

  document.addEventListener("click", function (e) {
    var btn = e.target.closest(".source-link-btn");
    if (!btn) return;
    e.preventDefault();
    e.stopPropagation();

    var path = btn.dataset.path;
    var blockId = btn.dataset.blockId;
    var markerSuffix = btn.dataset.markerSuffix || "init";
    if (!path && !blockId) return;

    // Open drawer
    document.body.classList.add("source-drawer-open");
    var toggleBtn = document.querySelector(".source-viewer-toggle");
    if (toggleBtn) toggleBtn.setAttribute("aria-expanded", "true");
    ensureDrawer();

    // Show loading state
    var body = document.querySelector(".source-drawer__code-body");
    if (body && !manifest) {
      body.innerHTML = '<div class="source-drawer__spinner" role="status">Loading...</div>';
    }

    // Wait for manifest, then open file
    loadManifestIfNeeded().then(function () {
      if (!document.body.classList.contains("source-drawer-open")) return;
      // Resolve path from blockId if not directly available
      if (!path && blockId) {
        path = findFileForBlock(blockId);
      }
      if (path) {
        openFile(path, blockId, markerSuffix);
      }
    });
  });
}

// ---------------------------------------------------------------------------
// Init + SPA support
// ---------------------------------------------------------------------------
initSourceViewer();
injectFileLinks();
registerFileLinkDelegate();

if (typeof document$ !== "undefined") {
  document$.subscribe(function () {
    // Auto-close drawer on SPA navigation to prevent context loss
    if (document.body.classList.contains("source-drawer-open")) {
      closeDrawer();
    }
    initSourceViewer();
    injectFileLinks();
  });
}

document.addEventListener("DOMContentLoaded", function () {
  initSourceViewer();
  injectFileLinks();
});
