# Unit conversions

units := import("units")

drive := units.convert(100, "km", "mi")
oven := units.convert(220, "celsius", "fahrenheit")

The drive is [units.format(drive, "mi")].
Set the oven to [units.format(oven, "f")].
[units.show(23, "kg", "lb")] fits the suitcase.
