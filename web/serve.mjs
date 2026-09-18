// Static files only: no WTF process, workspace API, or WebSocket server.
import http from "node:http";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { resolve, extname, sep } from "node:path";

const root = fileURLToPath(new URL(".", import.meta.url));
// The book and the example notes live beside web/ and share its worker and Wasm.
const siblings = { "/book": fileURLToPath(new URL("../book", import.meta.url)), "/examples": fileURLToPath(new URL("../examples", import.meta.url)), "/fonts": fileURLToPath(new URL("../fonts", import.meta.url)) };
const port = Number(process.env.WTF_WEB_PORT || 4173);
const types = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".json": "application/json", ".wtf": "text/plain", ".svg": "image/svg+xml", ".woff2": "font/woff2" };
const server = http.createServer(async (request, response) => {
  try {
    if (!["GET", "HEAD"].includes(request.method)) { response.writeHead(405).end(); return; }
    const path = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    const sibling = Object.entries(siblings).find(([prefix]) => path === prefix || path.startsWith(prefix + "/"));
    const base = sibling ? sibling[1] : root;
    const rest = sibling ? path.slice(sibling[0].length) : path;
    const file = resolve(base, `.${rest === "/" || rest === "" ? "/index.html" : rest}`);
    if (!file.startsWith(resolve(base) + sep) || !types[extname(file)]) { response.writeHead(403).end(); return; }
    const body = await readFile(file);
    response.writeHead(200, { "Content-Type": types[extname(file)], "Cache-Control": "no-cache", "X-Content-Type-Options": "nosniff" });
    response.end(request.method === "HEAD" ? undefined : body);
  } catch { response.writeHead(404).end("Not found. Run bash web/build.sh if the WebAssembly package is missing."); }
});
server.listen(port, "127.0.0.1", () => console.log(`WTF browser: http://127.0.0.1:${port} (static files only)`));
