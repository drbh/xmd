// esm.sh injects a Node process shim into the pinned Monaco bundle. Its fake
// versions.node makes Monaco detect Linux even on a Mac, breaking Cmd-click.
// Monaco guards every use of this import; absence selects its browser detection.
// The import-map scope limits this adapter to Monaco, not other CDN libraries.
export default undefined;
