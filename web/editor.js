import * as monaco from "https://esm.sh/monaco-editor@0.56.0?bundle";
export { monaco };

const point = p => ({ lineNumber: p.line + 1, column: p.character + 1 });
const position = p => ({ line: p.lineNumber - 1, character: p.column - 1 });
const range = r => new monaco.Range(r.start.line + 1, r.start.character + 1, r.end.line + 1, r.end.character + 1);
const lspRange = r => ({ start: { line: r.startLineNumber - 1, character: r.startColumn - 1 }, end: { line: r.endLineNumber - 1, character: r.endColumn - 1 } });
const markdown = value => ({ value: typeof value === "string" ? value : value?.value || "", isTrusted: false, supportHtml: false });
function emitter() {
  const listeners = new Set();
  return { event: listener => { listeners.add(listener); return { dispose: () => listeners.delete(listener) }; }, fire: () => { for (const listener of listeners) listener(); } };
}

export function createEditor(element, client) {
  monaco.languages.register({ id: "wtf", extensions: [".wtf"] });
  monaco.languages.setLanguageConfiguration("wtf", {
    brackets: [["[", "]"], ["(", ")"]],
    autoClosingPairs: [{ open: "[", close: "]" }, { open: "(", close: ")" }, { open: '"', close: '"' }],
    comments: { blockComment: ["<!--", "-->"] },
  });
  // The semantic tokens below come from Rust, not a second parser in JavaScript.
  monaco.editor.defineTheme("wtf-night", {
    base: "vs-dark", inherit: true,
    rules: [
      { token: "comment", foreground: "889781", fontStyle: "italic" },
      { token: "keyword", foreground: "F0A77D" }, { token: "number", foreground: "DABD90" },
      { token: "variable", foreground: "A8C7FA" },
      { token: "variable.declaration", foreground: "A8C7FA", fontStyle: "bold" },
      { token: "function", foreground: "C6A0F6" },
      { token: "property", foreground: "83D6CF" },
      { token: "property.declaration", foreground: "83D6CF", fontStyle: "bold" },
      { token: "decorator", foreground: "DEA2B8" },
      { token: "operator", foreground: "EDB486" },
      { token: "string", foreground: "CAD19B" },
      { token: "heading", foreground: "D6E8C2", fontStyle: "bold" },
      { token: "wtfMoney", foreground: "B4D98A", fontStyle: "bold" },
      { token: "wtfDate", foreground: "F2B3DA", fontStyle: "bold" },
      { token: "wtfTime", foreground: "91DCE8", fontStyle: "bold" },
      { token: "wtfDuration", foreground: "F4BA7A" },
      { token: "wtfRatio", foreground: "EAC080" },
      { token: "wtfBoolean", foreground: "C6A0F6" },
      { token: "wtfPunctuation", foreground: "85938B" },
      { token: "wtfCode", foreground: "A1AE9B" },
      { token: "wtfLink", foreground: "90BED8", fontStyle: "underline" },
      { token: "wtfCheckbox", foreground: "FFD580", fontStyle: "bold" },
      { token: "wtfCheckboxChecked", foreground: "91E6AC", fontStyle: "bold" },
      { token: "wtfTaskDone", foreground: "87A788", fontStyle: "strikethrough" },
      { token: "wtfDay", foreground: "FF9ECF", fontStyle: "bold" },
      { token: "wtfPlace", foreground: "E2C4FF" },
      { token: "wtfDetailKey", foreground: "9DB3A6", fontStyle: "italic" },
      { token: "wtfDepart", foreground: "FFB070" }, { token: "wtfDepart.declaration", foreground: "FFB070", fontStyle: "bold" },
      { token: "wtfArrive", foreground: "9CE8A0" }, { token: "wtfArrive.declaration", foreground: "9CE8A0", fontStyle: "bold" },
      { token: "wtfTransit", foreground: "8FCBFF" }, { token: "wtfTransit.declaration", foreground: "8FCBFF", fontStyle: "bold" },
      { token: "wtfStay", foreground: "C9A4FF" }, { token: "wtfStay.declaration", foreground: "C9A4FF", fontStyle: "bold" },
      { token: "wtfMeal", foreground: "FF8FA3" }, { token: "wtfMeal.declaration", foreground: "FF8FA3", fontStyle: "bold" },
      { token: "wtfVisit", foreground: "F6E38A" }, { token: "wtfVisit.declaration", foreground: "F6E38A", fontStyle: "bold" },
      { token: "wtfExplore", foreground: "7FE3D0" }, { token: "wtfExplore.declaration", foreground: "7FE3D0", fontStyle: "bold" },
    ],
    colors: {
      "editor.background": "#171b19", "editor.foreground": "#d4ded4", "editorLineNumber.foreground": "#66735f",
      "editorLineNumber.activeForeground": "#bacbad", "editorCursor.foreground": "#c1dea4",
      "editor.lineHighlightBackground": "#20271f", "editor.selectionBackground": "#35472f",
      "editorInlayHint.background": "#2b3826", "editorInlayHint.foreground": "#bad998",
      "editorCodeLens.foreground": "#9cab8e", "editorGutter.background": "#171b19",
      "editorWidget.background": "#20271f", "editorWidget.border": "#43523b",
      "editorHoverWidget.background": "#20271f", "editorHoverWidget.border": "#43523b",
      "editorSuggestWidget.background": "#20271f", "editorSuggestWidget.border": "#43523b",
      "editorSuggestWidget.selectedBackground": "#35472f", "editorError.foreground": "#efac9e",
      "menu.background": "#20271f", "menu.foreground": "#d4ded4", "menu.selectionBackground": "#35472f",
    },
  });
  const editor = monaco.editor.create(element, {
    theme: "wtf-night", automaticLayout: true, fontSize: 15, lineHeight: 27,
    fontFamily: '"Ioskeley Mono", ui-monospace, SFMono-Regular, Menlo, Consolas, monospace',
    minimap: { enabled: false }, scrollBeyondLastLine: false, wordWrap: "on",
    padding: { top: 24, bottom: 24 }, renderLineHighlight: "line", lineNumbersMinChars: 3,
    inlayHints: { enabled: "on", fontSize: 13 }, codeLens: true,
    "semanticHighlighting.enabled": true, quickSuggestions: true, wordBasedSuggestions: "off",
    parameterHints: { enabled: true }, tabSize: 2, fixedOverflowWidgets: true, formatOnType: true,
  });
  const hintChange = emitter(), lensChange = emitter(), tokenChange = emitter();
  const guarded = fn => async (...args) => {
    try { return await fn(...args); }
    catch (error) { client.error(error); return undefined; }
  };
  const command = (c, versions) => ({ id: "wtf.browser.execute", title: c.title, arguments: [c, versions] });
  monaco.editor.registerCommand("wtf.browser.execute", guarded(async (_accessor, c, versions) => {
    const result = await client.execute(c, versions);
    if (result?.edit) applyEdit(result.edit);
    if (result?.open) client.open(result.open);
  }));
  const toWorkspaceEdit = edit => ({ edits: (edit.documentChanges || []).flatMap(change => change.edits.map(e => ({
    resource: monaco.Uri.parse(change.textDocument.uri), versionId: change.textDocument.version,
    textEdit: { range: range(e.range), text: e.newText },
  }))) });
  function applyEdit(edit) {
    const changes = (edit.documentChanges || []).map(change => {
      const model = monaco.editor.getModel(monaco.Uri.parse(change.textDocument.uri));
      if (!model || model.getVersionId() !== change.textDocument.version) throw new Error("Note changed; request the action again.");
      const edits = change.edits.map(e => ({ range: range(e.range), text: e.newText }));
      if (edits.some(e => !model.validateRange(e.range).equalsRange(e.range))) throw new Error("Invalid edit range.");
      return { model, edits };
    });
    // Validate every target before making any changes. Monaco owns each note's undo history.
    for (const { model, edits } of changes) {
      model.pushStackElement();
      if (editor.getModel() === model) editor.executeEdits("wtf", edits);
      else model.pushEditOperations([], edits, () => null);
      model.pushStackElement();
    }
  }
  monaco.languages.registerInlayHintsProvider("wtf", {
    onDidChangeInlayHints: hintChange.event,
    provideInlayHints: guarded(async (model, requestedRange) => {
      const result = await client.analyze(model);
      return { hints: (result?.hints || []).filter(h => requestedRange.containsPosition(point(h.position))).map(h => ({
        position: point(h.position), label: h.label, kind: h.kind,
        tooltip: markdown(h.tooltip), paddingLeft: h.paddingLeft, paddingRight: h.paddingRight,
      })), dispose() {} };
    }),
  });
  monaco.languages.registerDocumentSemanticTokensProvider("wtf", {
    onDidChange: tokenChange.event,
    getLegend: () => client.legend,
    provideDocumentSemanticTokens: guarded(async model => ({ data: new Uint32Array((await client.analyze(model))?.tokens || []) })),
    releaseDocumentSemanticTokens() {},
  });
  monaco.languages.registerCodeLensProvider("wtf", {
    onDidChange: lensChange.event,
    provideCodeLenses: guarded(async model => {
      const result = await client.analyze(model);
      return { lenses: (result?.lenses || []).map(l => ({ range: range(l.range), command: command(l.command, result.versions) })), dispose() {} };
    }),
  });
  monaco.languages.registerHoverProvider("wtf", {
    provideHover: guarded(async (model, p) => {
      const result = await client.query(model, "hover", { position: position(p) });
      return result && { range: range(result.range), contents: [markdown(result.contents)] };
    }),
  });
  const kinds = ["Text", "Text", "Method", "Function", "Constructor", "Field", "Variable", "Class", "Interface", "Module", "Property", "Unit", "Value", "Enum", "Keyword", "Snippet", "Color", "File", "Reference", "Folder", "EnumMember", "Constant", "Struct", "Event", "Operator", "TypeParameter"];
  monaco.languages.registerCompletionItemProvider("wtf", {
    triggerCharacters: ["[", "@", "."],
    provideCompletionItems: guarded(async (model, p) => {
      const items = await client.query(model, "completion", { position: position(p) });
      return { suggestions: (items || []).map(item => ({
        label: item.label, kind: monaco.languages.CompletionItemKind[kinds[item.kind] || "Text"],
        detail: item.detail, documentation: markdown(item.documentation), sortText: item.sortText, filterText: item.filterText,
        insertText: item.textEdit?.newText || item.insertText || item.label,
        insertTextRules: item.insertTextFormat === 2 ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet : undefined,
        range: item.textEdit ? range(item.textEdit.range) : undefined,
      })) };
    }),
  });
  monaco.languages.registerSignatureHelpProvider("wtf", {
    signatureHelpTriggerCharacters: ["(", ","], signatureHelpRetriggerCharacters: [")"],
    provideSignatureHelp: guarded(async (model, p) => {
      const result = await client.query(model, "signature", { position: position(p) });
      return result && { value: { ...result, activeSignature: result.activeSignature || 0, activeParameter: result.activeParameter || 0,
        signatures: result.signatures.map(s => ({ ...s, documentation: markdown(s.documentation) })) }, dispose() {} };
    }),
  });
  monaco.languages.registerDocumentHighlightProvider("wtf", {
    provideDocumentHighlights: guarded(async (model, p) => (await client.query(model, "highlights", { position: position(p) }) || []).map(h => ({ ...h, range: range(h.range) }))),
  });
  const documentSymbol = s => ({
    name: s.name, detail: s.detail || "", kind: s.kind - 1, tags: s.tags || [],
    range: range(s.range), selectionRange: range(s.selectionRange),
    children: s.children?.map(documentSymbol),
  });
  monaco.languages.registerFoldingRangeProvider("wtf", {
    provideFoldingRanges: guarded(async model => ((await client.query(model, "folding")) || []).map(r => ({ start: r.startLine + 1, end: r.endLine + 1, kind: r.kind === "comment" ? monaco.languages.FoldingRangeKind.Comment : monaco.languages.FoldingRangeKind.Region }))),
  });
  monaco.languages.registerDocumentSymbolProvider("wtf", {
    displayName: "WTF",
    provideDocumentSymbols: guarded(async model => ((await client.query(model, "documentSymbols")) || []).map(documentSymbol)),
  });
  monaco.languages.registerDocumentFormattingEditProvider("wtf", {
    provideDocumentFormattingEdits: guarded(async model => ((await client.query(model, "formatting")) || []).map(e => ({ range: range(e.range), text: e.newText }))),
  });
  monaco.languages.registerOnTypeFormattingEditProvider("wtf", {
    autoFormatTriggerCharacters: ["\n", "|"],
    provideOnTypeFormattingEdits: guarded(async (model, p, ch) => ((await client.query(model, "onTypeFormatting", { position: position(p), ch })) || []).map(e => ({ range: range(e.range), text: e.newText }))),
  });
  const location = l => ({ uri: monaco.Uri.parse(l.uri), range: range(l.range) });
  monaco.languages.registerDefinitionProvider("wtf", {
    provideDefinition: guarded(async (model, p) => { const result = await client.query(model, "definition", { position: position(p) }); return result && location(result); }),
  });
  monaco.languages.registerReferenceProvider("wtf", {
    provideReferences: guarded(async (model, p, context) => {
      const result = await client.query(model, "references", { position: position(p) });
      return (context.includeDeclaration ? result || [] : (result || []).slice(1)).map(location);
    }),
  });
  monaco.languages.registerRenameProvider("wtf", {
    resolveRenameLocation: guarded(async (model, p) => {
      const result = await client.query(model, "prepareRename", { position: position(p) });
      return result && { range: range(result.range), text: result.placeholder };
    }),
    provideRenameEdits: async (model, p, newName) => {
      try { const result = await client.query(model, "rename", { position: position(p), newName }); return result && toWorkspaceEdit(result); }
      catch (error) { return { edits: [], rejectReason: error.message }; }
    },
  });
  monaco.languages.registerCodeActionProvider("wtf", {
    provideCodeActions: guarded(async (model, r, context) => {
      const result = await client.query(model, "actions", { range: lspRange(r) });
      return { actions: (result?.actions || []).filter(a => !context.only || a.kind === context.only || a.kind?.startsWith(context.only + ".")).map(a => ({
        title: a.title, kind: a.kind, edit: a.edit && toWorkspaceEdit(a.edit),
        command: a.command && command(a.command, result.versions),
      })), dispose() {} };
    }),
  }, { providedCodeActionKinds: ["quickfix", "refactor"] });
  monaco.languages.registerLinkProvider("wtf", {
    provideLinks: guarded(async model => ({ links: ((await client.analyze(model))?.links || []).map(l => ({ range: range(l.range), url: l.target, tooltip: l.tooltip })) })),
  });
  monaco.editor.registerLinkOpener({ open: resource => client.open(resource.toString()) });
  monaco.editor.registerEditorOpener({ openCodeEditor: (_source, resource, selection) => client.open(resource.toString(), selection) });
  let lastTokens = "", lastLenses = "", lastHints = "";
  function publish(model, snapshot) {
    monaco.editor.setModelMarkers(model, "wtf", snapshot.diagnostics.map(d => ({
      ...range(d.range), message: d.message, source: "wtf", code: d.code,
      severity: d.severity === 2 ? monaco.MarkerSeverity.Warning : monaco.MarkerSeverity.Error,
      relatedInformation: d.relatedInformation?.map(r => ({ resource: monaco.Uri.parse(r.location.uri), ...range(r.location.range), message: r.message })),
    })));
    const prefix = model.uri.toString();
    const tokens = prefix + JSON.stringify(snapshot.tokens), lenses = prefix + JSON.stringify([snapshot.lenses, snapshot.versions]), hints = prefix + JSON.stringify(snapshot.hints);
    if (tokens !== lastTokens) { lastTokens = tokens; tokenChange.fire(); }
    if (lenses !== lastLenses) { lastLenses = lenses; lensChange.fire(); }
    if (hints !== lastHints) { lastHints = hints; hintChange.fire(); }
  }
  return { editor, publish, applyEdit };
}
