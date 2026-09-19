// Serve only the assembled site. No sibling mounts or application backend.
import http from "node:http";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { resolve, extname, sep } from "node:path";

const root = fileURLToPath(new URL("./dist/", import.meta.url));
const prefix = `/${(process.env.WTF_WEB_BASE || "").replace(/^\/+|\/+$/g, "")}`.replace(/\/$/, "");
const port = Number(process.env.WTF_WEB_PORT || 4173);
const types = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".json": "application/json", ".wtf": "text/plain", ".svg": "image/svg+xml", ".woff2": "font/woff2", ".png": "image/png", ".webmanifest": "application/manifest+json" };
const server = http.createServer(async (request, response) => {
  try {
    if (!["GET", "HEAD"].includes(request.method)) { response.writeHead(405).end(); return; }
    let path = decodeURIComponent(new URL(request.url, "http://localhost").pathname);
    if (prefix && path !== prefix && !path.startsWith(prefix + "/")) { response.writeHead(404).end(); return; }
    path = path.slice(prefix.length) || "/";
    if (["/docs", "/book"].includes(path) || path === "/" && prefix && request.url === prefix) { response.writeHead(302, { Location: `${prefix}${path === "/" ? "/" : path + "/"}` }).end(); return; }
    const file = resolve(root, `.${path.endsWith("/") ? path + "index.html" : path}`);
    if (!file.startsWith(resolve(root) + sep) || !types[extname(file)]) { response.writeHead(403).end(); return; }
    const body = await readFile(file);
    response.writeHead(200, { "Content-Type": types[extname(file)], "Cache-Control": "no-cache", "X-Content-Type-Options": "nosniff" });
    response.end(request.method === "HEAD" ? undefined : body);
  } catch { response.writeHead(404).end("Not found. Run npm --prefix web run build first."); }
});
server.listen(port, "127.0.0.1", () => console.log(`WTF browser: http://127.0.0.1:${port}${prefix}/ (static files only)`));
