# web

from a [source checkout](../../README.md#develop), with Node.js, npm, and wasm-pack installed, run at the repository root:

```bash
make web
npm --prefix clients/web start
```

open http://127.0.0.1:4173. rebuild with `make web` after changes

`make wasm` builds only the browser package in `clients/web/pkg`. `make cloud` builds the site with its cloud backend module. see `make help` for all build targets
