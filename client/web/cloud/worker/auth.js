// Who is calling. Production identity comes from Cloudflare Access, which sits
// in front of /api/* and forwards a signed JWT; the Worker only verifies it.
// `DEV_AUTH=1` (wrangler dev only) substitutes a fixed development identity.

const jwks = new Map(); // team domain -> { keys, fetched }

export async function authenticate(request, env) {
  if (env.DEV_AUTH === "1") {
    const email = (request.headers.get("x-dev-user") || env.DEV_USER || "dev@example.com").toLowerCase();
    return { id: `dev:${email}`, email, name: email.split("@")[0] };
  }
  if (!env.ACCESS_TEAM_DOMAIN || !env.ACCESS_AUD) return { unconfigured: true };
  const token = request.headers.get("cf-access-jwt-assertion") || cookie(request, "CF_Authorization");
  if (!token) return null;
  try {
    const claims = await verify(token, env);
    return { id: claims.sub, email: String(claims.email).toLowerCase(), name: claims.name || null };
  } catch { return null; }
}

function cookie(request, name) {
  const header = request.headers.get("cookie") || "";
  const m = header.match(new RegExp(`(?:^|;\\s*)${name}=([^;]+)`));
  return m ? decodeURIComponent(m[1]) : null;
}
const b64 = s => Uint8Array.from(atob(s.replace(/-/g, "+").replace(/_/g, "/")), c => c.charCodeAt(0));
const decode = s => JSON.parse(new TextDecoder().decode(b64(s)));

async function keysFor(team) {
  const cached = jwks.get(team);
  if (cached && Date.now() - cached.fetched < 3_600_000) return cached.keys;
  const response = await fetch(`https://${team}/cdn-cgi/access/certs`);
  if (!response.ok) throw new Error("Cannot fetch Access keys");
  const { keys } = await response.json();
  jwks.set(team, { keys, fetched: Date.now() });
  return keys;
}
async function verify(token, env) {
  const [h, p, s] = token.split(".");
  if (!h || !p || !s) throw new Error("Malformed token");
  const header = decode(h), claims = decode(p);
  if (header.alg !== "RS256") throw new Error("Unexpected algorithm");
  const jwk = (await keysFor(env.ACCESS_TEAM_DOMAIN)).find(k => k.kid === header.kid);
  if (!jwk) throw new Error("Unknown key");
  const key = await crypto.subtle.importKey("jwk", jwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, false, ["verify"]);
  const ok = await crypto.subtle.verify("RSASSA-PKCS1-v1_5", key, b64(s), new TextEncoder().encode(`${h}.${p}`));
  if (!ok) throw new Error("Bad signature");
  const now = Math.floor(Date.now() / 1000);
  const aud = Array.isArray(claims.aud) ? claims.aud : [claims.aud];
  if (!aud.includes(env.ACCESS_AUD)) throw new Error("Wrong audience");
  if (claims.iss !== `https://${env.ACCESS_TEAM_DOMAIN}`) throw new Error("Wrong issuer");
  if (!(claims.exp > now) || (claims.nbf && claims.nbf > now + 60)) throw new Error("Expired");
  if (!claims.sub || !claims.email) throw new Error("Missing identity");
  return claims;
}
