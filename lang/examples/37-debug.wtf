# Debug values as JSON

Use debug(value) inside brackets to inspect a value as compact JSON.

## Simple values

[debug(true)]
[debug(42)]
[debug(null)]
[debug("Hello, Oaxaca!")]

## Nested objects and lists

trip := {
  place: "Oaxaca",
  packing: ["rain jacket", "walking shoes"],
  weather: {high: 25, rain: 35%},
  confirmed: true
}

[debug(trip)]
[debug(trip.weather)]
[debug(trip.weather.rain > 30%)]

## Values with units

details := {budget: $500, stay: 3d, arrival: 2026-11-20, rain: 35%}

[debug(details)]

Money, dates, durations, and ratios keep their type and units in the JSON.
You can also assign the JSON text to a name:

details_json := debug(details)

## Forecast objects

weather := forecast("Oaxaca", 2026-11-20, F)

[debug(weather)]

<!-- Run wtf refresh or use the ⟳ lookups lens to fill the forecast cache.
debug(weather) exposes high, low, rain, summary, and unit. The earlier sections
work without fetching anything. -->
