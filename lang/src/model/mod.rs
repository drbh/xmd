//! What a note is: the parser and the shapes it produces, the workspace of
//! notes, and the structures that live inside a note (tables, itineraries).
//! Parsing only: resolving an itinerary's dates and labels runs a module, so
//! it lives one layer up in `evaluate::itinerary`.
pub mod document;
pub mod imports;
pub mod itinerary;
pub mod paths;
pub mod session;
pub mod tables;
pub mod workspace;
