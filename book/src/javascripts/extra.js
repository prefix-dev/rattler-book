// ---------------------------------------------------------------------------
// Focus mode: toggle left nav visibility
// ---------------------------------------------------------------------------
// SVG icons: sidebar visible (menu) and sidebar hidden (menu-open)
// Panel-left icons (filled paths, Material style)
// Show nav: rectangle with left panel indicated
var ICON_SHOW_NAV = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M19 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V5a2 2 0 0 0-2-2m0 16H9V5h10v14M5 5h2v14H5V5Z"/></svg>';
// Hide nav: plain rectangle (no panel divider)
var ICON_HIDE_NAV = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M19 3H5a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2V5a2 2 0 0 0-2-2m0 16H5V5h14v14Z"/></svg>';

function initFocusMode() {
  // Restore saved preference
  if (localStorage.getItem("rb-focus-mode") === "true") {
    document.body.classList.add("focus-mode");
  }

  // Only add button once
  if (document.querySelector(".focus-toggle")) return;

  var palette = document.querySelector("[data-md-component=palette]");
  if (!palette) return;

  var btn = document.createElement("button");
  btn.className = "focus-toggle md-header__button md-icon";
  var on = document.body.classList.contains("focus-mode");
  btn.title = on ? "Show navigation" : "Focus mode";
  btn.innerHTML = on ? ICON_HIDE_NAV : ICON_SHOW_NAV;
  btn.addEventListener("click", function () {
    document.body.classList.toggle("focus-mode");
    var active = document.body.classList.contains("focus-mode");
    localStorage.setItem("rb-focus-mode", active);
    btn.innerHTML = active ? ICON_HIDE_NAV : ICON_SHOW_NAV;
    btn.title = active ? "Show navigation" : "Hide navigation";
  });

  palette.parentNode.insertBefore(btn, palette);
}

initFocusMode();

// ---------------------------------------------------------------------------
// Re-run after MkDocs Material instant (SPA) navigation
// ---------------------------------------------------------------------------
document.addEventListener("DOMContentLoaded", function () {
  initFocusMode();
});
if (typeof document$ !== "undefined") {
  document$.subscribe(function () {
    initFocusMode();
  });
}
