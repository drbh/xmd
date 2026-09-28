// The note extension, named once for the web client. The engine names it in
// lang/core/common/src/notes.rs; the docs app and the cloud worker import it from
// here, so renaming the format touches only these two places plus the editor
// settings listed in that Rust module.
export const EXTENSION = "x.md";
/** A note's file name from its stem: `noteFile("trip")` is "trip.x.md". */
export const noteFile = stem => `${stem}.${EXTENSION}`;
/** A name without a trailing note extension, in any case. */
export const noteStem = name => name.replace(new RegExp(`\\.${EXTENSION.replaceAll(".", "\\.")}$`, "i"), "");
