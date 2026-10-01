# Debug

trip := {
  place: "Oaxaca",
  packing: ["rain jacket", "walking shoes"],
  weather: {high: 25, rain: 35%},
  budget: $500
}

[debug(trip)]
[debug(trip.weather.rain > 30%)]
