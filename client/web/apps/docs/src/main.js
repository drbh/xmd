import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

mount(App, { target: document.getElementById("app") });

// Offline support: the built site ships a service worker beside the app. It
// is skipped in development, and the app works exactly the same without it.
// A worker serves one build; a newer one waits until the app takes it (a
// reload alone never swaps workers), and every other open tab is told once it
// has, so it can reload when that is safe.
if (!import.meta.env.DEV && "serviceWorker" in navigator && /^https?:$/.test(location.protocol)) {
  const container = navigator.serviceWorker;
  let previous = container.controller; // the first install claims a page without being a new build
  container.register("../sw.js").then(registration => {
    const offer = () => { if (registration.waiting && container.controller) window.dispatchEvent(new CustomEvent("xmd:update", { detail: registration })); };
    offer(); // installed on an earlier visit and still waiting
    registration.addEventListener("updatefound", () => {
      const worker = registration.installing;
      worker?.addEventListener("statechange", () => { if (worker.state === "installed") offer(); });
    });
    container.addEventListener("controllerchange", () => {
      if (previous) window.dispatchEvent(new CustomEvent("xmd:update", { detail: registration }));
      previous = container.controller;
    });
    // A tab left open looks for a newer build whenever it comes back.
    document.addEventListener("visibilitychange", () => { if (document.visibilityState === "visible") registration.update().catch(() => {}); });
  }).catch(() => { /* offline support is optional */ });
}
