// Serves the built site from ../dist and the accounts API under /api.
import { authenticate } from "./auth.js";
import { handle, json, HttpError } from "./api.js";

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
      return await handle(request, env, user);
    } catch (e) {
      if (e instanceof HttpError) return json({ error: e.message, ...e.extra }, e.status);
      console.error(e);
      return json({ error: "Something went wrong" }, 500);
    }
  },
};
