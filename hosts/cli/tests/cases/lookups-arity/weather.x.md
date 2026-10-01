# Forecasts

- single: [forecast("Oaxaca", 2026-11-20)] [forecast("Oaxaca", 2026-11-20, F)] [forecast("Oaxaca", 2026-11-20, "CELSIUS")] [forecast("Oaxaca", 2026-11-20, "FAHRENHEIT")]
- single arity: [forecast("Oaxaca")] [forecast("Oaxaca", 2026-11-20, F, C)]
- single types: [forecast(1, 2026-11-20)] [forecast("Oaxaca", "soon")] [forecast("Oaxaca", 2026-11-20, K)] [forecast("Oaxaca", 2026-11-20, 3)]
- range: [forecast_range("Oaxaca", 2026-11-20, 2026-11-20)] [forecast_range("Oaxaca", 2026-11-20, 2026-11-21)]
- range arity: [forecast_range("Oaxaca", 2026-11-20)] [forecast_range("Oaxaca", 2026-11-20, 2026-11-21, F, C)]
- range types: [forecast_range(3, 2026-11-20, 2026-11-21)] [forecast_range("Oaxaca", 2026-11-20, "later")] [forecast_range("Oaxaca", 2026-11-20, 2040-01-01)] [forecast_range("Oaxaca", 2026-11-20, 2026-11-20, K)]
