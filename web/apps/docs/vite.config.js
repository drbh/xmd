import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  base: "./",
  plugins: [svelte()],
  build: {
    outDir: "../../dist/docs",
    emptyOutDir: true,
    // All apps load the release's single library and Wasm artifact.
    rollupOptions: { external: id => id.startsWith("@wtf/web") },
  },
});
