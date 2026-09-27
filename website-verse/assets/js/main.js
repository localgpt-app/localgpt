/* LocalGPT Verse website — shared interactions. No dependencies. */
(function () {
  "use strict";

  var reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  /* ---------- Mobile nav ---------- */
  var toggle = document.querySelector(".nav-toggle");
  var links = document.querySelector(".nav-links");
  if (toggle && links) {
    toggle.addEventListener("click", function () {
      var open = links.classList.toggle("open");
      toggle.setAttribute("aria-expanded", open ? "true" : "false");
    });
  }

  /* ---------- Hero world cycler ----------
     The same rule as the app: the chrome never changes; only the accent does.
     Cycles the hero (and the page accent) through the eight built-in worlds. */
  var WORLDS = [
    { id: "ember-flats",    name: "Ember Flats",    accent: "#ffb38a", skyTop: "#ffd9b0", skyBottom: "#c9836f", fog: "#9e6b66", ground: "#7a4f63" },
    { id: "velvet-circuit", name: "Velvet Circuit", accent: "#62f5ff", skyTop: "#241054", skyBottom: "#0b0722", fog: "#3d1a57", ground: "#0d0b2a" },
    { id: "tide-gardens",   name: "Tide Gardens",   accent: "#a8d8de", skyTop: "#12333e", skyBottom: "#0b0c11", fog: "#2d6a72", ground: "#12333e" },
    { id: "glass-expanse",  name: "Glass Expanse",  accent: "#8ef4ff", skyTop: "#231b2d", skyBottom: "#0b0722", fog: "#332947", ground: "#161125" },
    { id: "cinder-reach",   name: "Cinder Reach",   accent: "#e86a4a", skyTop: "#8b402d", skyBottom: "#4d2520", fog: "#5c3833", ground: "#38201c" },
    { id: "mirage-circuit", name: "Mirage Circuit", accent: "#b7f5ff", skyTop: "#87aac9", skyBottom: "#4d5d73", fog: "#8c9eb3", ground: "#4a5361" },
    { id: "abyss-terraces", name: "Abyss Terraces", accent: "#6fa8b8", skyTop: "#06141b", skyBottom: "#03060a", fog: "#0f2930", ground: "#061317" },
    { id: "dawn-expanse",   name: "Dawn Expanse",   accent: "#ffd9b8", skyTop: "#d6bab0", skyBottom: "#927a73", fog: "#b8a199", ground: "#61524f" }
  ];

  var stage = document.querySelector(".world-stage");
  if (stage) {
    var chip = document.querySelector(".hero-world .world-name");
    var index = 0;

    var apply = function (world) {
      stage.style.setProperty("--h-sky-top", world.skyTop);
      stage.style.setProperty("--h-sky-bottom", world.skyBottom);
      stage.style.setProperty("--h-fog", world.fog);
      stage.style.setProperty("--h-ground", world.ground);
      document.documentElement.style.setProperty("--accent", world.accent);
      if (chip) chip.textContent = world.name;
    };

    apply(WORLDS[0]);

    if (!reduceMotion) {
      window.setInterval(function () {
        index = (index + 1) % WORLDS.length;
        apply(WORLDS[index]);
      }, 6000);
    }
  }

  /* ---------- Scroll reveal ---------- */
  var revealables = document.querySelectorAll(".reveal");
  if (revealables.length) {
    if (reduceMotion || !("IntersectionObserver" in window)) {
      revealables.forEach(function (el) { el.classList.add("in"); });
    } else {
      var revealObserver = new IntersectionObserver(function (entries) {
        entries.forEach(function (entry) {
          if (entry.isIntersecting) {
            entry.target.classList.add("in");
            revealObserver.unobserve(entry.target);
          }
        });
      }, { threshold: 0.12 });
      revealables.forEach(function (el) { revealObserver.observe(el); });
    }
  }
})();
