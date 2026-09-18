// Pure score arithmetic, shared by the sports link module and by notes.
module := {api: 1, id: "scores", kind: "library", inputs: []}

// Name the side that won, or "draw" when the scores are level.
resolve := fn(home_score, away_score) => (
  if(
    home_score > away_score,
    "home",
    if(away_score > home_score, "away", "draw")
  )
)

// Report the absolute score difference.
margin := fn(home_score, away_score) => (
  if(home_score > away_score, home_score - away_score, away_score - home_score)
)

// Render a compact scoreline with the home side first.
scoreline := fn(home, home_score, away, away_score) => (
  home + " " + text(home_score) + " – " + away + " " + text(away_score)
)

// Describe the result from the home team's point of view.
outcome := fn(home_score, away_score) => (
  coalesce(
    get(
      {home: "home win", away: "away win", draw: "draw"},
      resolve(home_score, away_score)
    ),
    "unknown"
  )
)
