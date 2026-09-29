# 5. put a note on your own page

no build step, nothing on your server. the engine loads from the hosted
app into a worker in the page

```html
<link rel="stylesheet" href="https://xmd.dholtz.com/lib/theme/style.css">
<div id="note"></div>
<script type="module">
  import { mountEditor } from "https://xmd.dholtz.com/lib/adapters/contenteditable.js";
  mountEditor(document.querySelector("#note"), { source: "rent := $900\nfood := $250\nTotal [rent + food]\n" });
</script>
```

`mount` gives a read-only view, `render` static html, `onChange` tells you
what people typed. https://xmd.dholtz.com/embed/ is that page, live

next: [6. when not to use it](06-limits.md)
