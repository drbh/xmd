[budget] := minimize(30m * calls + 1h * visits)
| constraint | expression               |
| ---------- | ------------------------ |
| coverage   | calls + 3 * visits >= 10 |
