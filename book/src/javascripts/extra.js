// Force margin notes open on wide screens, collapse on narrow
function updateMarginNotes() {
  var wide = window.matchMedia("(min-width: 1400px)").matches;
  document.querySelectorAll("details.margin-note").forEach(function (d) {
    d.open = wide;
  });
}
updateMarginNotes();
window.matchMedia("(min-width: 1400px)").addEventListener("change", updateMarginNotes);
