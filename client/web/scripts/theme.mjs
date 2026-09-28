// The neutral palette supplies the HTML theme and the Zed adapter.
import { readFile, writeFile } from "node:fs/promises";
const theme = new URL("../theme/", import.meta.url);
const rules = JSON.parse(await readFile(new URL("palette.json", theme), "utf8"));
// The palette is tuned for dark paper. Light paper keeps each token's hue and
// pulls its lightness down so the same semantic colors read on white.
function onLight(hex) {
  const [r, g, b] = [1, 3, 5].map(i => parseInt(hex.slice(i, i + 2), 16) / 255);
  const max = Math.max(r, g, b), min = Math.min(r, g, b), d = max - min;
  let h = 0;
  if (d) h = max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  const s = d ? d / (1 - Math.abs(max + min - 1)) : 0;
  const l = (max + min) / 2;
  // Near-white tones are prose-like (headings); they become ink rather than a tint.
  const plain = l > 0.78 && s < 0.5;
  const light = plain ? 0.14 : d < 0.08 ? 0.42 : 0.36, sat = plain ? 0 : Math.min(0.72, s * 0.85);
  const c = (1 - Math.abs(2 * light - 1)) * sat, x = c * (1 - Math.abs(h % 2 - 1)), m = light - c / 2;
  const [R, G, B] = [[c, x, 0], [x, c, 0], [0, c, x], [0, x, c], [x, 0, c], [c, 0, x]][Math.floor(h)];
  return "#" + [R, G, B].map(v => Math.round((v + m) * 255).toString(16).padStart(2, "0")).join("");
}
const lightCss = rules.map(rule => `.xmd-light .xmd .t-${rule.token_type}${(rule.token_modifiers || []).map(m => `.${m}`).join("")} { color: ${onLight(rule.foreground_color)}; }`).join("\n");
const css = rules.map(rule => {
  const selector = `.xmd .t-${rule.token_type}${(rule.token_modifiers || []).map(m => `.${m}`).join("")}`;
  const properties = [`color: ${rule.foreground_color}`];
  if (rule.font_weight) properties.push(`font-weight: ${rule.font_weight}`);
  if (rule.font_style) properties.push(`font-style: ${rule.font_style}`);
  if (rule.underline) properties.push("text-decoration-line: underline");
  if (rule.strikethrough) properties.push("text-decoration-line: line-through");
  return `${selector} { ${properties.join("; ")}; }`;
}).join("\n");
await writeFile(new URL("style.css", theme), `${await readFile(new URL("base.css", theme), "utf8")}\n${css}\n${lightCss}\n${await readFile(new URL("print.css", theme), "utf8")}`);
await writeFile(new URL("../../ide/zed/languages/xmd/semantic_token_rules.json", import.meta.url), await readFile(new URL("palette.json", theme)));
