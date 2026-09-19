import { mount } from "svelte";
import App from "./App.svelte";
import "./app.css";

mount(App, { target: document.getElementById("app") });

// Offline support: the built site ships a service worker beside the app. It
// is skipped in development, and the app works exactly the same without it.
if (!import.meta.env.DEV && "serviceWorker" in navigator && /^https?:$/.test(location.protocol)) {
  navigator.serviceWorker.register("../sw.js").then(registration => {
    registration.addEventListener("updatefound", () => {
      const worker = registration.installing;
      worker?.addEventListener("statechange", () => {
        if (worker.state === "installed" && navigator.serviceWorker.controller) window.dispatchEvent(new CustomEvent("wtf:update", { detail: registration }));
      });
    });
  }).catch(() => { /* offline support is optional */ });
}
