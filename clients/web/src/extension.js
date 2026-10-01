// The extensions, named once for the web client. The engine names them in
// core/common/src/notes.rs; the docs app and the cloud worker import them
// from here, so renaming the format touches only these two places plus the
// editor settings listed in that Rust module. A working note is `.x.md`; a file
// that only defines names for others to import is `.xmd`. Both read the same.
export const EXTENSION = "x.md";
export const LIBRARY_EXTENSION = "xmd";
/** A note's file name from its stem: `noteFile("trip")` is "trip.x.md". */
export const noteFile = stem => `${stem}.${EXTENSION}`;
/** A name without a trailing note or library extension, in any case. */
export const noteStem = name => name.replace(new RegExp(`\\.(${[EXTENSION, LIBRARY_EXTENSION].map(e => e.replaceAll(".", "\\.")).join("|")})$`, "i"), "");
