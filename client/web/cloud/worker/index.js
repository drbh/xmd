// Serves the built site from client/web/dist and the accounts API under /api.
import { routePartykitRequest } from "partyserver";
import { authenticate, authenticateKey } from "./auth.js";
import { handle, json, HttpError, roleOf, ensureUser, publicDocument } from "./api.js";
export { Room } from "./room.js";

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    // Link sharing: a view-only document for anyone with its token, no sign-in.
    const shared = /\/public\/v1\/([^/]+)$/.exec(url.pathname);
    if (shared && request.method === "GET") {
      try { return await publicDocument(env.DB, shared[1]); }
      catch (e) { return e instanceof HttpError ? json({ error: e.message }, e.status) : json({ error: "Something went wrong" }, 500); }
    }
    // The sync API lives outside the browser sign-in and takes an API key instead.
    const sync = /^(.*)\/sync\/v1(\/.*)?$/.exec(url.pathname);
    if (sync) {
      try {
        const user = await authenticateKey(request, env);
        if (!user) return json({ error: "A valid API key is required" }, 401, { "www-authenticate": "Bearer" });
        const rewritten = new URL(request.url);
        rewritten.pathname = `${sync[1]}/api${sync[2] || ""}`;
        return await handle(new Request(rewritten, request), env, user);
      } catch (e) {
        if (e instanceof HttpError) return json({ error: e.message, ...e.extra }, e.status);
        console.error(e);
        return json({ error: "Something went wrong" }, 500);
      }
    }
    // Static files are served before this code runs; lib/ gets its CORS headers from dist/_headers.
    if (!/\/api(\/|$)/.test(url.pathname)) return env.ASSETS.fetch(request);
    try {
      const user = await authenticate(request, env);
      if (user?.unconfigured) return json({ error: "Sign-in is not configured for this deployment" }, 503);
      if (!user) return json({ error: "Sign in to continue" }, 401);
      if (url.pathname.endsWith("/api/login")) {
        // Access has already authenticated the browser; land it back in the app.
        const next = url.searchParams.get("next") || "/";
        return Response.redirect(new URL(/^\/(?!\/)/.test(next) ? next : "/", url).href, 302);
      }
      if (url.pathname.endsWith("/api/logout")) {
        const to = env.ACCESS_TEAM_DOMAIN ? `https://${env.ACCESS_TEAM_DOMAIN}/cdn-cgi/access/logout` : new URL("/", url).href;
        return Response.redirect(to, 302);
      }
      // Live editing: /api/rooms/room/<id> upgrades to the document's room once the role is known.
      const live = await routePartykitRequest(request, env, {
        prefix: "api/rooms",
        async onBeforeConnect(req, lobby) {
          await ensureUser(env.DB, user);
          const { doc, role } = await roleOf(env.DB, user, lobby.name);
          if (!doc || !role) return json({ error: "Document not found" }, 404);
          req.headers.set("x-xmd-role", role);
          req.headers.set("x-xmd-email", user.email);
        },
        onBeforeRequest: () => json({ error: "Rooms accept WebSocket connections only" }, 400),
      });
      if (live) return live;
      return await handle(request, env, user);
    } catch (e) {
      if (e instanceof HttpError) return json({ error: e.message, ...e.extra }, e.status);
      console.error(e);
      return json({ error: "Something went wrong" }, 500);
    }
  },
};
