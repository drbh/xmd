oaxaca_f := forecast("Oaxaca", 2026-11-20, F)
pack_rain_jacket := oaxaca_f.rain > 30%

We should [pack_rain_jacket] for Oaxaca.

mexico_city := forecast("Mexico City", 2026-11-20, F)
pack_rain_jacket_mexico_city := mexico_city.rain > 30%

We should [pack_rain_jacket_mexico_city] for Mexico City.

## A week of seasonal estimates

For November 20–26, each bar is one day, from left to right. Temperature charts
scale to their own range; rain charts use a fixed 0–100% scale.

oaxaca_week := forecast_range("Oaxaca", 2026-11-20, 2026-11-26, F)

Oaxaca highs °F: [sparkline(oaxaca_week.high)]
Oaxaca lows °F: [sparkline(oaxaca_week.low)]
Oaxaca rain: [sparkline(oaxaca_week.rain, 0%, 100%)]

mexico_city_week := forecast_range("Mexico City", 2026-11-20, 2026-11-26, F)

Mexico City highs °F: [sparkline(mexico_city_week.high)]
Mexico City lows °F: [sparkline(mexico_city_week.low)]
Mexico City rain: [sparkline(mexico_city_week.rain, 0%, 100%)]

<!-- Use the ⟳ lookups lens to refresh the whole week. These long-range
temperatures and rain probabilities are seasonal estimates. -->
