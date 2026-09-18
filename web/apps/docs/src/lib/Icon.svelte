<script>
  // Small stroke icons for the chrome; the document itself never uses them.
  let { name, size = 18 } = $props();
  const paths = {
    undo: "M9 14 4 9l5-5M4 9h10a6 6 0 0 1 0 12h-3",
    redo: "m15 14 5-5-5-5M20 9H10a6 6 0 0 0 0 12h3",
    print: "M6 9V3h12v6M6 18H4a2 2 0 0 1-2-2v-5a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v5a2 2 0 0 1-2 2h-2M6 14h12v7H6z",
    bold: "M7 4h6a4 4 0 0 1 0 8H7zM7 12h7a4 4 0 0 1 0 8H7z",
    italic: "M10 4h8M6 20h8M14 4l-4 16",
    strike: "M4 12h16M8 7a4 3 0 0 1 4-3h1a4 3 0 0 1 3 2M16 17a4 3 0 0 1-4 3h-1a4 3 0 0 1-3-2",
    code: "m8 8-4 4 4 4M16 8l4 4-4 4M14 4l-4 16",
    link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1.5 1.5M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1.5-1.5",
    bullet: "M9 6h11M9 12h11M9 18h11M4 6h.01M4 12h.01M4 18h.01",
    number: "M10 6h10M10 12h10M10 18h10M4 5h1v4M4 9h2M4 14h2l-2 3h2M4 18",
    checklist: "M10 6h10M10 12h10M10 18h10M3 6l1.5 1.5L7 5M3 12l1.5 1.5L7 11M3 18l1.5 1.5L7 17",
    indent: "M3 6h18M11 12h10M11 18h10M3 10l4 3-4 3",
    outdent: "M3 6h18M11 12h10M11 18h10M7 10l-4 3 4 3",
    search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zM20 20l-4-4",
    outline: "M4 6h16M4 12h10M4 18h13",
    close: "M6 6l12 12M18 6 6 18",
    up: "m6 15 6-6 6 6",
    down: "m6 9 6 6 6-6",
    more: "M12 6h.01M12 12h.01M12 18h.01",
    plus: "M12 5v14M5 12h14",
    doc: "M6 3h8l5 5v13H6zM14 3v5h5M9 13h7M9 17h7",
    check: "m5 12 4 4L19 7",
    cloud: "M7 18a4 4 0 0 1-.5-8A6 6 0 0 1 18 9a4.5 4.5 0 0 1-1 9z",
    sun: "M12 3v2M12 19v2M3 12h2M19 12h2M5.6 5.6l1.4 1.4M17 17l1.4 1.4M5.6 18.4 7 17M17 7l1.4-1.4M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z",
    moon: "M20 14A8 8 0 0 1 10 4a8 8 0 1 0 10 10z",
    warning: "M12 4 2 20h20zM12 10v4M12 17h.01",
    play: "M7 5v14l11-7z",
    table: "M4 5h16v14H4zM4 10h16M4 15h16M10 5v14",
    timer: "M12 7v5l3 2M12 21a8 8 0 1 0 0-16 8 8 0 0 0 0 16zM9 2h6",
    back: "m10 19-7-7 7-7M3 12h18",
    upload: "M12 16V4M6 10l6-6 6 6M4 20h16",
    expand: "m9 6 6 6-6 6",
  };
</script>

<svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d={paths[name]}></path></svg>
