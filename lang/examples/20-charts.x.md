# Plain-text charts

Gauges and sparklines are drawn with Unicode blocks, so every editor shows them.

review := countdown(40m, 16m)

## Launch :launch
- [x] Draft
- [x] Screenshots
- [ ] Demo
- [ ] Changelog

sales := table
| month | units |
| ----- | ----- |
| Jan   | 12    |
| Feb   | 18    |
| Mar   | 9     |
| Apr   | 24    |

total_units := sum(sales, units)

Sales trend: [sparkline([12, 18, 9, 24])]
Rain on a 0–100% scale: [sparkline([20%, 40%, 30%, 70%], 0%, 100%)]

<!-- The countdown and heading inlays carry a gauge; hover them for the bar with
a percentage. Hover the units column or total_units for a sparkline. -->
