# Weather and quotes

External data lives in a cache that only xmd refresh or the ⟳ lookups
lens fills; nothing is fetched while you type.

oaxaca := forecast("Oaxaca", 2026-11-20)
oaxaca_f := forecast("Oaxaca", 2026-11-20, F)
pack_rain_jacket := oaxaca.rain > 30%

Landing day looks like [oaxaca], or [oaxaca_f] in Fahrenheit.

Inspect the forecast fields as JSON: [debug(oaxaca_f)].

12:shares
nvda := quote(NVDA) * shares

Holding [shares] shares is worth [nvda].

<!-- Daily forecasts cover 16 days including today. Later dates use seasonal
outlooks for up to about 7 months, labeled as estimates. Seasonal .rain is the
share of available model runs predicting more than 0.1 mm of precipitation
(including snow) that day; it needs at least two valid runs. Uppercase names
such as NVDA or F are codes, not references. Hover a value to see each lookup's
age and source. -->
