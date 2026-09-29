// Keep a directory of notes in step with a folder in the hosted docs app.
// A bundled plugin: run it with `xmd run sync DIR --url https://…
// [--folder NAME] [--key KEY] [--dry-run] [--watch] [--interval SECONDS]`.
//
// Each file is compared three ways against the copy last synced, kept in
// DIR/.xmd-sync/base: unchanged on one side means the other side wins;
// changed on both is merged line by line, and a real conflict is written
// beside the note as NAME.conflict.x.md without touching either version.
// The docs app addresses documents by file name within a folder, so relative
// imports mean the same thing on both sides. This module only decides; the
// `xmd run` host performs the requests it returns.
module := {api: 1, id: "sync", kind: "command", inputs: []}

_state_dir := ".xmd-sync"

// A record with one field set, replacing any earlier value.
_set := fn(record, key, value) => (
  object(concat(filter(entries(record), fn(e) => e.key != key), [{key: key, value: value}]))
)

// A record without one field.
_unset := fn(record, key) => (
  object(filter(entries(record), fn(e) => e.key != key))
)

// The note's name without `.x.md`.
_stem := fn(name) => (
  slice(name, 0, length(name) - 5)
)

// Where a conflicted merge is written beside the note.
_conflict := fn(stem) => (
  stem + ".conflict.x.md"
)

// The copy of a note as it was last synced.
_base := fn(name) => (
  _state_dir + "/base/" + name
)

// A note this command syncs: `.x.md`, not a conflict file, not hidden.
_syncable := fn(name) => (
  ends_with(name, ".x.md")
  && length(name) > 5
  && !ends_with(name, ".conflict.x.md")
  && !starts_with(name, ".")
)

// The characters of a line, without the empty edges `split` leaves.
_chars := fn(line) => (
  filter(split(line, ""), fn(c) => c != "")
)

// A heading's text: the line without its leading `#` marks.
_unhash := fn(line) => (
  fold(
    _chars(line),
    {marks: true, text: ""},
    fn(acc, c) => if(acc.marks && c == "#", acc, {marks: false, text: acc.text + c})
  ).text
)

// Whether every character is a letter, digit or underscore.
_word := fn(value) => (
  length(value) > 0
  && length(filter(_chars(value), fn(c) => !contains("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_", c))) == 0
)

// A heading without a trailing ` :name` label.
_unlabel := fn(title) => let({parts: split(title, " :"), last: length(parts) - 1}, (
  if(
    last > 0 && _word(get(parts, last)),
    trim(join(slice(parts, 0, last), " :")),
    title
  )
))

// A line that is a heading with text after its marks.
_heading := fn(line) => (
  starts_with(line, "#") && starts_with(_unhash(line), " ") && trim(_unhash(line)) != ""
)

// The display name the web app would derive: the first heading, else the file name.
_title := fn(text, fallback) => let({headings: filter(split(text, "\n"), _heading)}, (
  if(length(headings) > 0, _unlabel(trim(_unhash(get(headings, 0)))), fallback)
))

// Why a response is not a success, or null when it is one.
_problem := fn(r) => (
  if(
    !r.ok, r.error,
    r.status >= 200 && r.status < 300, null,
    // Only a failed answer is a record that may carry an error.
    let({error: get(get(r, "json"), "error")}, if(
      error == null, "The web app answered " + text(r.status),
      r.status == 401, error + " (create one in the web app under API keys)",
      error
    ))
  )
)

// A request to the docs app's sync API.
_api := fn(s, method, path, body) => (
  {
    kind: "http",
    method: method,
    url: s.url + "/sync/v1" + path,
    headers: {Authorization: "Bearer " + s.key},
    json: body
  }
)

// Save a document: its name follows its first heading, as in the web app.
_save := fn(s, id, stem, text, version) => (
  _api(s, "PUT", "/documents/" + id, {name: _title(text, stem), file: stem, text: text, version: version, folder: s.folder_id})
)

// A step that asks for more, remembering whether anything has been reported.
_ask := fn(s, requests, report) => (
  {state: _said(s, report), requests: requests, report: report, done: false}
)

// Stop with an error message.
_fail := fn(message) => (
  {error: message, done: true}
)

// Record the outcome of one file and note that something was said.
_said := fn(s, report) => (
  _set(s, "said", get(s, "said") == true || length(report) > 0)
)

// Finish the run: save the manifest, and in --watch mode start again later.
_finish := fn(s, report) => (
  {
    state: s,
    requests: if(s.dry, [], [{kind: "write", path: _state_dir + "/manifest.json", json: s.manifest}]),
    report: concat(report, if(!s.watch && get(s, "said") != true && length(report) == 0, ["Up to date."], [])),
    done: true,
    repeat_after: if(s.watch, s.interval, null)
  }
)

// Read the next file's local text and its last synced copy, or finish.
_next := fn(s, report) => let({name: get(s.queue, 0)}, (
  if(
    length(s.queue) == 0,
    _finish(_said(s, report), report),
    _ask(
      _set(_set(_set(_said(s, report), "stage", "decide"), "name", name), "queue", slice(s.queue, 1, length(s.queue))),
      [{kind: "read", path: name}, {kind: "read", path: _base(name)}],
      report
    )
  )
))

// Continue once the effects of one file are done, stopping if a required one failed.
_after := fn(s, results, report) => let({failed: filter(slice(results, 0, s.check), fn(r) => !r.ok)}, (
  if(length(failed) > 0, _fail(get(failed, 0).error), _next(s, report))
))

// Perform effects, then move on to the next file. The first `check` requests
// must succeed; the rest (removing an old base copy) may fail harmlessly.
_then := fn(s, requests, check, report) => (
  _ask(_set(_set(s, "stage", "after"), "check", check), requests, report)
)

// The manifest with one file's remote identity recorded.
_track := fn(s, name, id, version) => (
  _set(s, "manifest", _set(s.manifest, "files", _set(s.manifest.files, name, {id: id, version: version})))
)

// The manifest without one file.
_forget := fn(s, name) => (
  _set(s, "manifest", _set(s.manifest, "files", _unset(s.manifest.files, name)))
)

// What a dry run says it would do.
_describe := fn(action, stem) => (
  get(
    {
      create: "would create " + stem + " in the web app",
      pull: "would pull " + stem,
      push: "would push " + stem,
      merge: "would merge " + stem + " (changed on both sides)",
      delete_remote: "would remove " + stem + " from the web app",
      trash_local: "would move " + stem + " to trash"
    },
    action.kind
  )
)

// Write the merge, or a conflict file when both sides changed the same lines.
_merged := fn(s, name, merged, remote) => (
  if(
    merged.clean,
    _ask(
      _set(_set(_set(s, "stage", "merged"), "text", merged.text), "doc", remote),
      [{kind: "write", path: name, text: merged.text}, _save(s, remote.id, _stem(name), merged.text, remote.version)],
      []
    ),
    _then(
      _track(s, name, remote.id, remote.version),
      [{kind: "write", path: _conflict(_stem(name)), text: merged.text}, {kind: "write", path: _base(name), text: remote.text}],
      2,
      ["conflict in " + _stem(name) + ": resolve " + _conflict(_stem(name)) + ", copy it over " + name + ", and delete it"]
    )
  )
)

// Carry out one decided action.
_perform := fn(s, action) => let({said: _describe(action, _stem(s.name))}, (
  if(s.dry, _next(s, if(said == null, [], [said])), match(action.kind,
    "nothing", _next(s, []),
    "forget", _then(_forget(s, s.name), [{kind: "remove", path: _base(s.name)}], 0, []),
    "create", _ask(_set(_set(s, "stage", "create"), "text", action.local), [{kind: "uuid"}], []),
    "adopt", _then(
      _track(s, s.name, action.remote.id, action.remote.version),
      [{kind: "write", path: _base(s.name), text: action.remote.text}],
      1,
      []
    ),
    "pull", _then(
      _track(s, s.name, action.remote.id, action.remote.version),
      [{kind: "write", path: s.name, text: action.remote.text}, {kind: "write", path: _base(s.name), text: action.remote.text}],
      2,
      ["pulled " + _stem(s.name)]
    ),
    "push", _ask(
      _set(_set(_set(_set(s, "stage", "pushed"), "text", action.local), "doc", action.remote), "base", action.base),
      [_save(s, action.remote.id, _stem(s.name), action.local, action.remote.version)],
      []
    ),
    "merge", _merged(s, s.name, merge3(action.base, action.local, action.remote.text), action.remote),
    "delete_remote", _ask(_set(s, "stage", "deleted"), [_api(s, "DELETE", "/documents/" + action.remote.id, null)], []),
    // trash_local
    _then(
      _forget(s, s.name),
      [{kind: "move", from: s.name, to: _state_dir + "/trash/" + s.name}, {kind: "remove", path: _base(s.name)}],
      1,
      ["moved " + _stem(s.name) + " to " + _state_dir + "/trash (it was removed in the web app)"]
    )
  ))
))

// Decide what a file needs from its local text, last synced copy and remote
// listing. `remote` carries text only once it has been fetched.
_decide := fn(s, local, base, remote) => let({
  synced: get(s.manifest.files, s.name),
  moved: synced != null && remote != null && synced.version != remote.version
}, (
  if(
    local == null && remote == null, {kind: "forget"},
    // New to this directory.
    synced == null && remote == null, {kind: "create", local: local},
    synced == null && local == null, {kind: "pull", remote: remote},
    synced == null && local == remote.text, {kind: "adopt", remote: remote},
    synced == null, {kind: "merge", base: "", local: local, remote: remote},
    // Synced before: deleted here, deleted there, or changed on either side.
    local == null && moved, {kind: "pull", remote: remote},
    local == null, {kind: "delete_remote", remote: remote},
    remote == null && base == local, {kind: "trash_local"},
    remote == null, {kind: "create", local: local},
    base == local && moved, {kind: "pull", remote: remote},
    base == local, {kind: "nothing"},
    moved, {kind: "merge", base: coalesce(base, ""), local: local, remote: remote},
    {kind: "push", local: local, remote: remote, base: coalesce(base, "")}
  )
))

// Whether deciding this file needs the remote text: a remote document that is
// new to this directory or has changed since the last sync. Pushes, trashing
// and deletions work from the listing alone.
_needs_text := fn(s, remote) => let({synced: get(s.manifest.files, s.name)}, (
  remote != null && (synced == null || synced.version != remote.version)
))

// The remote documents in the synced folder, keyed by file name.
_remote := fn(documents, folder_id) => (
  object(
    map(
      filter(documents, fn(d) => d.folder == folder_id && d.role != "viewer"),
      fn(d) => {key: d.file + ".x.md", value: {id: d.id, version: d.version}}
    )
  )
)

// Every name either side or the manifest knows, in order.
_names := fn(s) => (
  sort_by(
    filter(
      concat(
        filter(s.local, _syncable),
        concat(map(entries(s.remote), fn(e) => e.key), map(entries(s.manifest.files), fn(e) => e.key))
      ),
      fn(n) => true
    ),
    fn(n) => n
  )
)

// Each name once.
_unique := fn(names) => (
  fold(names, [], fn(out, n) => if(contains(out, n), out, concat(out, [n])))
)

// Plan the files once the folder and the remote listing are known.
_plan := fn(s, documents, report) => let({known: _set(s, "remote", _remote(documents, s.folder_id))}, (
  _next(_set(known, "queue", _unique(_names(known))), report)
))

// The saved manifest, or an empty one.
_manifest := fn(result) => let({saved: get(result, "json")}, (
  if(
    result.ok,
    {folder_id: get(saved, "folder_id"), files: coalesce(get(saved, "files"), {})},
    {folder_id: null, files: {}}
  )
))

// Drop one trailing slash from an address.
_trim_slash := fn(url) => (
  if(ends_with(url, "/"), slice(url, 0, length(url) - 1), url)
)

// A flag given as text, or null when absent or bare.
_flag := fn(flags, name) => let({value: get(flags, name)}, (
  if(type(value) == "Text", value, null)
))

step := fn(ctx) => (
  if(
    ctx.state == null,
    _ask(
      {stage: "setup"},
      [
        {kind: "read", path: _state_dir + "/config.json", json: true},
        {kind: "read", path: _state_dir + "/manifest.json", json: true},
        {kind: "env", name: "XMD_API_KEY"},
        {kind: "list", path: "."}
      ],
      []
    ),
    _stage(ctx.state, ctx.results, ctx.args.flags, ctx.dir)
  )
)

// Stop on a failed answer from the web app, else take the next step.
_unless_failed := fn(result, next) => let({problem: _problem(result)}, (
  if(problem != null, _fail(problem), next())
))

// Everything after the first step, by the stage the state records.
_stage := fn(s, r, flags, dir) => let({first: get(r, 0)}, match(s.stage,
  "setup", _setup(r, flags, dir),
  "key", _key(s, r),
  "listed", _listed(s, r),
  "folder", _ask(_set(s, "stage", "folder_created"), [_api(s, "PUT", "/folders/" + first.value, {name: s.folder})], []),
  "folder_created", _unless_failed(first, fn() => let({folder: get(first, "json")}, _plan(
    _set(_set(s, "folder_id", get(folder, "id")), "manifest", _set(s.manifest, "folder_id", get(folder, "id"))),
    s.documents,
    ["created folder \"" + get(folder, "name") + "\""]
  ))),
  // The per-file stages.
  "decide", _file(
    s,
    if(contains(filter(s.local, _syncable), s.name) && first.ok, get(first, "text"), null),
    if(get(r, 1).ok, get(get(r, 1), "text"), null)
  ),
  "fetched", _unless_failed(first, fn() => _perform(s, _decide(s, s.local_text, s.base_text, get(first, "json")))),
  "create", _ask(_set(_set(s, "stage", "created"), "id", first.value), [_save(s, first.value, _stem(s.name), s.text, null)], []),
  "created", _unless_failed(first, fn() => _then(
    _track(s, s.name, s.id, coalesce(get(get(first, "json"), "version"), 1)),
    [{kind: "write", path: _base(s.name), text: s.text}],
    1,
    ["created " + _stem(s.name) + " in the web app"]
  )),
  "pushed", _pushed(s, first),
  "merged", _merge_saved(s, r),
  "deleted", _unless_failed(first, fn() => _then(
    _forget(s, s.name),
    [{kind: "remove", path: _base(s.name)}],
    0,
    ["removed " + _stem(s.name) + " from the web app (it was deleted here)"]
  )),
  _after(s, r, [])
))

// Settle the address, folder and key, then list the web app's side.
_setup := fn(r, flags, dir) => let({
  saved: get(get(r, 0), "json"),
  given: coalesce(_flag(flags, "url"), get(saved, "url"), ""),
  folder: coalesce(
    _flag(flags, "folder"),
    if(coalesce(get(saved, "folder"), "") == "", null, get(saved, "folder")),
    if(dir == "", null, dir),
    "Notes"
  )
}, (
  if(
    given == "",
    _fail("Pass --url the first time, e.g. --url https://xmd.example.com"),
    let({url: _trim_slash(given)}, _ask(
      {
        stage: "key",
        url: url,
        folder: folder,
        manifest: _manifest(get(r, 1)),
        local: coalesce(get(get(r, 3), "files"), []),
        dry: get(flags, "dry_run") == true,
        watch: get(flags, "watch") == true,
        interval: coalesce(_flag(flags, "interval"), "5"),
        said: false
      },
      [
        {kind: "write", path: _state_dir + "/config.json", json: {url: url, folder: folder}},
        {kind: "credential", scope: url, set: coalesce(_flag(flags, "key"), get(r, 2).value)}
      ],
      []
    ))
  )
))

// With a key in hand, fetch the folders (the first time) and the documents.
_key := fn(s, r) => let({key: get(r, 1).value, keyed: _set(s, "key", key)}, (
  if(
    key == null,
    _fail("No API key: pass --key once, or set XMD_API_KEY"),
    _ask(
      _set(keyed, "stage", "listed"),
      concat(
        if(s.manifest.folder_id == null, [_api(keyed, "GET", "/folders", null)], []),
        [_set(_api(keyed, "GET", "/documents", null), "pick", ["id", "name", "file", "version", "role", "folder"])]
      ),
      []
    )
  )
))

// Find or create the folder, then plan the files.
_listed := fn(s, r) => let({failed: filter(r, fn(x) => _problem(x) != null)}, (
  if(
    length(failed) > 0,
    _fail(_problem(get(failed, 0))),
    if(
      s.manifest.folder_id != null,
      _plan(_set(s, "folder_id", s.manifest.folder_id), get(get(r, 0), "json"), []),
      _folder(s, get(get(r, 0), "json"), get(get(r, 1), "json"))
    )
  )
))

// The named folder, matched without regard to case.
_folder := fn(s, folders, documents) => let({found: get(filter(folders, fn(f) => lower(f.name) == lower(s.folder)), 0)}, (
  if(
    found != null,
    if(
      found.role == "viewer",
      _fail("You can only view the folder \"" + found.name + "\""),
      _plan(_set(_set(s, "folder_id", found.id), "manifest", _set(s.manifest, "folder_id", found.id)), documents, [])
    ),
    if(
      s.dry,
      {state: s, report: ["would create folder \"" + s.folder + "\""], done: true},
      _ask(_set(_set(s, "stage", "folder"), "documents", documents), [{kind: "uuid"}], [])
    )
  )
))

// Decide one file, fetching the remote text first when the decision needs it.
_file := fn(s, local, base) => let({conflict: _conflict(_stem(s.name)), remote: get(s.remote, s.name)}, (
  if(
    contains(s.local, conflict),
    _next(s, [_stem(s.name) + ": still has " + conflict + "; resolve it to continue syncing this file"]),
    if(
      _needs_text(s, remote),
      _ask(
        _set(_set(_set(s, "stage", "fetched"), "local_text", local), "base_text", base),
        [_api(s, "GET", "/documents/" + remote.id, null)],
        []
      ),
      _perform(s, _decide(s, local, base, remote))
    )
  )
))

// A push either lands, or finds the document moved on and merges against it.
_pushed := fn(s, result) => let({
  answer: get(result, "json"),
  current: get(answer, "current"),
  problem: _problem(result)
}, (
  if(
    result.ok && get(result, "status") == 409 && current != null,
    _merged(s, s.name, merge3(s.base, s.text, current.text), current),
    problem != null,
    _fail(problem),
    _then(
      _track(s, s.name, s.doc.id, coalesce(get(answer, "version"), s.doc.version + 1)),
      [{kind: "write", path: _base(s.name), text: s.text}],
      1,
      ["pushed " + _stem(s.name)]
    )
  )
))

// After writing a clean merge, record it, or report that the web app keeps changing.
_merge_saved := fn(s, r) => let({written: get(r, 0), saved: get(r, 1), problem: _problem(saved)}, (
  if(
    !written.ok,
    _fail(written.error),
    saved.ok && get(saved, "status") == 409,
    _next(s, [_stem(s.name) + " keeps changing in the web app; try again"]),
    problem != null,
    _fail(problem),
    _then(
      _track(s, s.name, s.doc.id, coalesce(get(get(saved, "json"), "version"), s.doc.version + 1)),
      [{kind: "write", path: _base(s.name), text: s.text}],
      1,
      ["merged " + _stem(s.name)]
    )
  )
))
