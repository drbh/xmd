# Dates and relative dates

2026-11-20:departure
days_left := departure - today()
return_date := departure + 10d

Leaving in [days_left], back on [return_date].
today() is [today()] and date("next Friday") is [date("next Friday")].

- [ ] Book the hotel @due(departure - 14d)
- [ ] Pack @due(tomorrow)

<!-- Try: put the cursor on tomorrow and use Freeze relative date. -->
