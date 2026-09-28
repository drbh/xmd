week := forecast_range("Oaxaca", 2026-11-20, 2026-11-22, F)
Highs: [sparkline(week.high)]
Rain: [sparkline(week.rain, 0%, 100%)]
Celsius: [debug(forecast_range("Oaxaca", 2026-11-20, 2026-11-22).high)]
One day: [sparkline(forecast_range("Oaxaca", 2026-11-20, 2026-11-20).high)]
Inline only: [sparkline(forecast_range("Oaxaca", 2026-11-24, 2026-11-25).high)]
