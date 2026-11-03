// Home page behaviour.
(function () {
  // The primary buttons' shimmer pauses while they are off-screen.
  if ("IntersectionObserver" in window) {
    var watch = new IntersectionObserver(function (entries) {
      entries.forEach(function (entry) { entry.target.classList.toggle("is-offscreen", !entry.isIntersecting); });
    });
    document.querySelectorAll(".button.primary").forEach(function (button) { watch.observe(button); });
  }

  // One-click copy for the install commands. The buttons stay hidden without the Clipboard API.
  if (!navigator.clipboard) return;
  document.querySelectorAll(".copy-btn[data-copy]").forEach(function (button) {
    var label = button.querySelector("span");
    var icon = button.querySelector("use");
    var timer;
    button.hidden = false;
    button.addEventListener("click", function () {
      navigator.clipboard.writeText(button.dataset.copy).then(function () {
        button.classList.add("is-copied");
        label.textContent = "Copied";
        icon.setAttribute("href", "#icon-check");
        clearTimeout(timer);
        timer = setTimeout(function () {
          button.classList.remove("is-copied");
          label.textContent = "Copy";
          icon.setAttribute("href", "#icon-copy");
        }, 1600);
      });
    });
  });
})();
