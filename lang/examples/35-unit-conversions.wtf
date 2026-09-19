# Unit conversions

The bundled units library converts between temperature, distance, mass, volume,
speed, area, and data units.

units := import("units")

drive := units.convert(100, "km", "mi")
oven := units.convert(220, "celsius", "fahrenheit")
suitcase := units.convert(23, "kg", "lb")
download := units.convert(2.5, "gb", "mb")

The drive is [drive] miles, or [units.format(drive, "mi")] rounded.
Set the oven to [units.format(oven, "f")] and pack under [units.format(suitcase, "lb")].
That download is [units.format(download, "mb")].

[units.dimension("knot")]
[units.show(60, "mph", "kph")]

<!-- Try units.convert(1, "cup", "ml"), or ask for something impossible like
units.convert(1, "km", "kg") to see the error inline. -->
