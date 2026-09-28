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
_unlabel := fn(title) => (
  if(
    length(split(title, " :")) > 1 && _word(get(split(title, " :"), length(split(title, " :")) - 1)),
    trim(join(slice(split(title, " :"), 0, length(split(title, " :")) - 1), " :")),
    title
  )
)

// A line that is a heading with text after its marks.
_heading := fn(line) => (
  starts_with(line, "#") && starts_with(_unhash(line), " ") && trim(_unhash(line)) != ""
)

// The display name the web app would derive: the first heading, else the file name.
_title := fn(text, fallback) => (
  if(
    length(filter(split(text, "\n"), _heading)) > 0,
    _unlabel(trim(_unhash(get(filter(split(text, "\n"), _heading), 0)))),
    fallback
  )
)

// Why a response is not a success, or null when it is one.
_problem := fn(r) => (
  if(
    !r.ok,
    r.error,
    if(
      r.status >= 200 && r.status < 300,
      null,
      if(
        get(get(r, "json"), "error") == null,
        "The web app answered " + text(r.status),
        if(
          r.status == 401,
          get(get(r, "json"), "error") + " (create one in the web app under API keys)",
          get(get(r, "json"), "error")
        )
      )
    )
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
_next := fn(s, report) => (
  if(
    length(s.queue) == 0,
    _finish(_said(s, report), report),
    _ask(
      _set(_set(_set(_said(s, report), "stage", "decide"), "name", get(s.queue, 0)), "queue", slice(s.queue, 1, length(s.queue))),
      [{kind: "read", path: get(s.queue, 0)}, {kind: "read", path: _base(get(s.queue, 0))}],
      report
    )
  )
)

// Continue once the effects of one file are done, stopping if a required one failed.
_after := fn(s, results, report) => (
  if(
    length(filter(slice(results, 0, s.check), fn(r) => !r.ok)) > 0,
    _fail(get(filter(slice(results, 0, s.check), fn(r) => !r.ok), 0).error),
    _next(s, report)
  )
)

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
_perform := fn(s, action) => (
  if(
    s.dry,
    _next(s, if(_describe(action, _stem(s.name)) == null, [], [_describe(action, _stem(s.name))])),
    if(
      action.kind == "nothing",
      _next(s, []),
      if(
        action.kind == "forget",
        _then(_forget(s, s.name), [{kind: "remove", path: _base(s.name)}], 0, []),
        if(
          action.kind == "create",
          _ask(_set(_set(s, "stage", "create"), "text", action.local), [{kind: "uuid"}], []),
          if(
            action.kind == "adopt",
            _then(_track(s, s.name, action.remote.id, action.remote.version), [{kind: "write", path: _base(s.name), text: action.remote.text}], 1, []),
            if(
              action.kind == "pull",
              _then(
                _track(s, s.name, action.remote.id, action.remote.version),
                [{kind: "write", path: s.name, text: action.remote.text}, {kind: "write", path: _base(s.name), text: action.remote.text}],
                2,
                ["pulled " + _stem(s.name)]
              ),
              if(
                action.kind == "push",
                _ask(
                  _set(_set(_set(_set(s, "stage", "pushed"), "text", action.local), "doc", action.remote), "base", action.base),
                  [_save(s, action.remote.id, _stem(s.name), action.local, action.remote.version)],
                  []
                ),
                if(
                  action.kind == "merge",
                  _merged(s, s.name, merge3(action.base, action.local, action.remote.text), action.remote),
                  if(
                    action.kind == "delete_remote",
                    _ask(_set(s, "stage", "deleted"), [_api(s, "DELETE", "/documents/" + action.remote.id, null)], []),
                    _then(
                      _forget(s, s.name),
                      [{kind: "move", from: s.name, to: _state_dir + "/trash/" + s.name}, {kind: "remove", path: _base(s.name)}],
                      1,
                      ["moved " + _stem(s.name) + " to " + _state_dir + "/trash (it was removed in the web app)"]
                    )
                  )
                )
              )
            )
          )
        )
      )
    )
  )
)

// Decide what a file needs from its local text, last synced copy and remote
// listing. `remote` carries text only once it has been fetched.
_decide := fn(s, local, base, remote) => (
  if(
    local == null && remote == null,
    {kind: "forget"},
    if(
      get(s.manifest.files, s.name) == null,
      if(
        remote == null,
        {kind: "create", local: local},
        if(
          local == null,
          {kind: "pull", remote: remote},
          if(local == remote.text, {kind: "adopt", remote: remote}, {kind: "merge", base: "", local: local, remote: remote})
        )
      ),
      if(
        local == null,
        if(
          get(s.manifest.files, s.name).version != remote.version,
          {kind: "pull", remote: remote},
          {kind: "delete_remote", remote: remote}
        ),
        if(
          remote == null,
          if(base == local, {kind: "trash_local"}, {kind: "create", local: local}),
          if(
            base == local,
            if(get(s.manifest.files, s.name).version != remote.version, {kind: "pull", remote: remote}, {kind: "nothing"}),
            if(
              get(s.manifest.files, s.name).version != remote.version,
              {kind: "merge", base: coalesce(base, ""), local: local, remote: remote},
              {kind: "push", local: local, remote: remote, base: coalesce(base, "")}
            )
          )
        )
      )
    )
  )
)

// Whether deciding this file needs the remote text: a remote document that is
// new to this directory or has changed since the last sync. Pushes, trashing
// and deletions work from the listing alone.
_needs_text := fn(s, remote) => (
  remote != null
  && (get(s.manifest.files, s.name) == null || get(s.manifest.files, s.name).version != remote.version)
)

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
_plan := fn(s, documents, report) => (
  _next(
    _set(_set(s, "remote", _remote(documents, s.folder_id)), "queue", _unique(_names(_set(s, "remote", _remote(documents, s.folder_id))))),
    report
  )
)

// The saved manifest, or an empty one.
_manifest := fn(result) => (
  if(
    result.ok,
    {folder_id: get(get(result, "json"), "folder_id"), files: coalesce(get(get(result, "json"), "files"), {})},
    {folder_id: null, files: {}}
  )
)

// Drop one trailing slash from an address.
_trim_slash := fn(url) => (
  if(ends_with(url, "/"), slice(url, 0, length(url) - 1), url)
)

// A flag given as text, or null when absent or bare.
_flag := fn(flags, name) => (
  if(type(get(flags, name)) == "Text", get(flags, name), null)
)

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

// Everything after the first step, by the stage the state records.
_stage := fn(s, r, flags, dir) => (
  if(
    s.stage == "setup",
    _setup(r, flags, dir),
    if(
      s.stage == "key",
      _key(s, r),
      if(
        s.stage == "listed",
        _listed(s, r),
        if(
          s.stage == "folder",
          _ask(_set(s, "stage", "folder_created"), [_api(s, "PUT", "/folders/" + get(r, 0).value, {name: s.folder})], []),
          if(
            s.stage == "folder_created",
            if(
              _problem(get(r, 0)) != null,
              _fail(_problem(get(r, 0))),
              _plan(
                _set(_set(s, "folder_id", get(get(get(r, 0), "json"), "id")), "manifest", _set(s.manifest, "folder_id", get(get(get(r, 0), "json"), "id"))),
                s.documents,
                ["created folder \"" + get(get(get(r, 0), "json"), "name") + "\""]
              )
            ),
            _file_stage(s, r)
          )
        )
      )
    )
  )
)

// Settle the address, folder and key, then list the web app's side.
_setup := fn(r, flags, dir) => (
  if(
    coalesce(_flag(flags, "url"), get(get(get(r, 0), "json"), "url"), "") == "",
    _fail("Pass --url the first time, e.g. --url https://xmd.example.com"),
    _ask(
      {
        stage: "key",
        url: _trim_slash(coalesce(_flag(flags, "url"), get(get(get(r, 0), "json"), "url"))),
        folder: coalesce(_flag(flags, "folder"), if(coalesce(get(get(get(r, 0), "json"), "folder"), "") == "", null, get(get(get(r, 0), "json"), "folder")), if(dir == "", null, dir), "Notes"),
        manifest: _manifest(get(r, 1)),
        local: coalesce(get(get(r, 3), "files"), []),
        dry: get(flags, "dry_run") == true,
        watch: get(flags, "watch") == true,
        interval: coalesce(_flag(flags, "interval"), "5"),
        said: false
      },
      [
        {
          kind: "write",
          path: _state_dir + "/config.json",
          json: {
            url: _trim_slash(coalesce(_flag(flags, "url"), get(get(get(r, 0), "json"), "url"))),
            folder: coalesce(_flag(flags, "folder"), if(coalesce(get(get(get(r, 0), "json"), "folder"), "") == "", null, get(get(get(r, 0), "json"), "folder")), if(dir == "", null, dir), "Notes")
          }
        },
        {
          kind: "credential",
          scope: _trim_slash(coalesce(_flag(flags, "url"), get(get(get(r, 0), "json"), "url"))),
          set: coalesce(_flag(flags, "key"), get(r, 2).value)
        }
      ],
      []
    )
  )
)

// With a key in hand, fetch the folders (the first time) and the documents.
_key := fn(s, r) => (
  if(
    get(r, 1).value == null,
    _fail("No API key: pass --key once, or set XMD_API_KEY"),
    _ask(
      _set(_set(s, "stage", "listed"), "key", get(r, 1).value),
      concat(
        if(s.manifest.folder_id == null, [_api(_set(s, "key", get(r, 1).value), "GET", "/folders", null)], []),
        [
          {
            kind: "http",
            method: "GET",
            url: s.url + "/sync/v1/documents",
            headers: {Authorization: "Bearer " + get(r, 1).value},
            pick: ["id", "name", "file", "version", "role", "folder"]
          }
        ]
      ),
      []
    )
  )
)

// Find or create the folder, then plan the files.
_listed := fn(s, r) => (
  if(
    length(filter(r, fn(x) => _problem(x) != null)) > 0,
    _fail(_problem(get(filter(r, fn(x) => _problem(x) != null), 0))),
    if(
      s.manifest.folder_id != null,
      _plan(_set(s, "folder_id", s.manifest.folder_id), get(get(r, 0), "json"), []),
      _folder(s, get(get(r, 0), "json"), get(get(r, 1), "json"))
    )
  )
)

// The named folder, matched without regard to case.
_folder := fn(s, folders, documents) => (
  if(
    length(filter(folders, fn(f) => lower(f.name) == lower(s.folder))) > 0,
    if(
      get(filter(folders, fn(f) => lower(f.name) == lower(s.folder)), 0).role == "viewer",
      _fail("You can only view the folder \"" + get(filter(folders, fn(f) => lower(f.name) == lower(s.folder)), 0).name + "\""),
      _plan(
        _set(
          _set(s, "folder_id", get(filter(folders, fn(f) => lower(f.name) == lower(s.folder)), 0).id),
          "manifest",
          _set(s.manifest, "folder_id", get(filter(folders, fn(f) => lower(f.name) == lower(s.folder)), 0).id)
        ),
        documents,
        []
      )
    ),
    if(
      s.dry,
      {state: s, report: ["would create folder \"" + s.folder + "\""], done: true},
      _ask(_set(_set(s, "stage", "folder"), "documents", documents), [{kind: "uuid"}], [])
    )
  )
)

// The per-file stages.
_file_stage := fn(s, r) => (
  if(
    s.stage == "decide",
    _file(s, if(contains(filter(s.local, _syncable), s.name) && get(r, 0).ok, get(get(r, 0), "text"), null), if(get(r, 1).ok, get(get(r, 1), "text"), null)),
    if(
      s.stage == "fetched",
      if(
        _problem(get(r, 0)) != null,
        _fail(_problem(get(r, 0))),
        _perform(s, _decide(s, s.local_text, s.base_text, get(get(r, 0), "json")))
      ),
      if(
        s.stage == "create",
        _ask(_set(_set(s, "stage", "created"), "id", get(r, 0).value), [_save(s, get(r, 0).value, _stem(s.name), s.text, null)], []),
        if(
          s.stage == "created",
          if(
            _problem(get(r, 0)) != null,
            _fail(_problem(get(r, 0))),
            _then(
              _track(s, s.name, s.id, coalesce(get(get(get(r, 0), "json"), "version"), 1)),
              [{kind: "write", path: _base(s.name), text: s.text}],
              1,
              ["created " + _stem(s.name) + " in the web app"]
            )
          ),
          if(
            s.stage == "pushed",
            _pushed(s, get(r, 0)),
            if(
              s.stage == "merged",
              _merge_saved(s, r),
              if(
                s.stage == "deleted",
                if(
                  _problem(get(r, 0)) != null,
                  _fail(_problem(get(r, 0))),
                  _then(_forget(s, s.name), [{kind: "remove", path: _base(s.name)}], 0, ["removed " + _stem(s.name) + " from the web app (it was deleted here)"])
                ),
                _after(s, r, [])
              )
            )
          )
        )
      )
    )
  )
)

// Decide one file, fetching the remote text first when the decision needs it.
_file := fn(s, local, base) => (
  if(
    contains(s.local, _conflict(_stem(s.name))),
    _next(s, [_stem(s.name) + ": still has " + _conflict(_stem(s.name)) + "; resolve it to continue syncing this file"]),
    if(
      _needs_text(s, get(s.remote, s.name)),
      _ask(
        _set(_set(_set(s, "stage", "fetched"), "local_text", local), "base_text", base),
        [_api(s, "GET", "/documents/" + get(s.remote, s.name).id, null)],
        []
      ),
      _perform(s, _decide(s, local, base, get(s.remote, s.name)))
    )
  )
)

// A push either lands, or finds the document moved on and merges against it.
_pushed := fn(s, result) => (
  if(
    result.ok && get(result, "status") == 409 && get(get(result, "json"), "current") != null,
    _merged(s, s.name, merge3(s.base, s.text, get(get(result, "json"), "current").text), get(get(result, "json"), "current")),
    if(
      _problem(result) != null,
      _fail(_problem(result)),
      _then(
        _track(s, s.name, s.doc.id, coalesce(get(get(result, "json"), "version"), s.doc.version + 1)),
        [{kind: "write", path: _base(s.name), text: s.text}],
        1,
        ["pushed " + _stem(s.name)]
      )
    )
  )
)

// After writing a clean merge, record it, or report that the web app keeps changing.
_merge_saved := fn(s, r) => (
  if(
    !get(r, 0).ok,
    _fail(get(r, 0).error),
    if(
      get(r, 1).ok && get(get(r, 1), "status") == 409,
      _next(s, [_stem(s.name) + " keeps changing in the web app; try again"]),
      if(
        _problem(get(r, 1)) != null,
        _fail(_problem(get(r, 1))),
        _then(
          _track(s, s.name, s.doc.id, coalesce(get(get(get(r, 1), "json"), "version"), s.doc.version + 1)),
          [{kind: "write", path: _base(s.name), text: s.text}],
          1,
          ["merged " + _stem(s.name)]
        )
      )
    )
  )
)
