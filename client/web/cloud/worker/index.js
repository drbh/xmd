// Serves the built site from client/web/dist and the accounts API under /api.
import { routePartykitRequest } from "partyserver";
import { authenticate } from "./auth.js";
import { handle, json, HttpError, roleOf, ensureUser } from "./api.js";
export { Room } from "./room.js";

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
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
          req.headers.set("x-wtf-role", role);
          req.headers.set("x-wtf-email", user.email);
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
