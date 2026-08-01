    (function () {
      var conceptButtons = Array.from(document.querySelectorAll("[data-concept]"));
      var concepts = Array.from(document.querySelectorAll(".concept"));
      var toast = document.getElementById("toast");
      var toastTimer;

      function showToast(message) {
        toast.textContent = message;
        toast.classList.add("show");
        window.clearTimeout(toastTimer);
        toastTimer = window.setTimeout(function () {
          toast.classList.remove("show");
        }, 1600);
      }

      function switchConcept(id) {
        conceptButtons.forEach(function (button) {
          button.classList.toggle("active", button.dataset.concept === id);
        });
        concepts.forEach(function (concept) {
          concept.classList.toggle("active", concept.id === "concept-" + id);
        });
      }

      conceptButtons.forEach(function (button) {
        button.addEventListener("click", function () {
          switchConcept(button.dataset.concept);
        });
      });

      document.addEventListener("keydown", function (event) {
        if (event.key === "1" || event.key === "2" || event.key === "3") {
          switchConcept({ "1": "a", "2": "b", "3": "c" }[event.key]);
        }
      });

      document.getElementById("a-collapse").addEventListener("click", function () {
        document.getElementById("a-shell").classList.toggle("collapsed");
      });

      document.querySelectorAll(".a-nav-item").forEach(function (item) {
        item.addEventListener("click", function () {
          document.querySelectorAll(".a-nav-item").forEach(function (nav) {
            nav.classList.remove("active");
          });
          item.classList.add("active");
          var title = item.querySelector(".nav-copy");
          if (title) {
            document.getElementById("a-page-title").textContent = title.textContent;
          }
        });
      });

      document.querySelectorAll(".b-nav-item").forEach(function (item) {
        item.addEventListener("click", function () {
          document.querySelectorAll(".b-nav-item").forEach(function (nav) {
            nav.classList.remove("active");
          });
          item.classList.add("active");
        });
      });

      document.querySelectorAll(".b-side-item").forEach(function (item) {
        item.addEventListener("click", function () {
          document.querySelectorAll(".b-side-item").forEach(function (nav) {
            nav.classList.remove("active");
          });
          item.classList.add("active");
        });
      });

      document.querySelectorAll(".c-rail-button").forEach(function (item) {
        item.addEventListener("click", function () {
          document.querySelectorAll(".c-rail-button").forEach(function (nav) {
            nav.classList.remove("active");
          });
          item.classList.add("active");
        });
      });

      document.querySelectorAll(".c-tab").forEach(function (item) {
        item.addEventListener("click", function () {
          document.querySelectorAll(".c-tab").forEach(function (tab) {
            tab.classList.remove("active");
          });
          item.classList.add("active");
          showToast("检查器已切换到「" + item.textContent + "」");
        });
      });

      document.querySelectorAll(".js-node").forEach(function (node) {
        node.addEventListener("click", function () {
          document.querySelectorAll(".js-node").forEach(function (item) {
            item.classList.remove("selected");
          });
          node.classList.add("selected");
          var title = node.querySelector("strong");
          document.getElementById("c-inspector-title").textContent = title.textContent;
        });
      });

      document.querySelectorAll(".js-menu").forEach(function (item) {
        item.addEventListener("click", function () {
          var text = item.innerText.replace(/\s+/g, " ").trim();
          if (text) {
            showToast("已选择「" + text.slice(0, 22) + "」");
          }
        });
      });
    }());
