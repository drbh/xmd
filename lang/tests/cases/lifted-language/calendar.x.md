impossible := import("lifted").date("2026-02-29", "%F")
bad_time := import("lifted").datetime("2026-09-18 25:00", "%F %H:%M", now())
parsed := source(import("lifted").datetime("2026-09-18 09:30", "%F %H:%M", now()))
