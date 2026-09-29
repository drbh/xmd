# Sports scores

Open this folder as a workspace so .xmd/modules.json activates sports.xmd and
scores.xmd. The sports link module resolves ESPN game pages.

https://www.espn.com/nba/game/_/gameId/401584896:game

The score is [game.home_score] to [game.away_score], a margin of [game.margin].
[game.winner] won, and the clock says [game.status].

scores := import("scores")
called := scores.resolve(112, 108)
spread := scores.margin(112, 108)

A home score of 112 against 108 resolves to [called] by [spread].

<!-- Before a refresh the link reads "◌ score (refresh)" and the properties
report that nothing is cached. Run the refresh action on the link (it shells out
to curl), then hover it for the fetched-at summary. -->
