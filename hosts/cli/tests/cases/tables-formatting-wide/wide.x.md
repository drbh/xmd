# Wide cells

places := table
| name | city | n |
|---|:---:|---:|
| 東京タワー | "東京" | 1 |
| ❤️ | "Zürich" | 22 |
| "🍎🍎" | "café" | 3 |
| "e" | "ｆｕｌｌ" | 4 |

choices := table
| item | cost | take? | n# |
| :--- | ---: | :---: | --- |
| Tea | $3 | | |
| "Big pie" | $5 | yes | 2 |

  indented := table
  | a | b |
  | --- | --- |
  | 1 | 2 |
	| 333 | 4 |

pipes := table
| label | formula |
| --- | --- |
| "a\|b" | 1 |
| "x|y" | 2 |
| "q" | [1 + 2] |

[plan] := maximize(x + y)
| constraint | expression |
|:---|---:|
| cap | x + y <= 10 |
| 9bad | x >= 0 |
| three | y >= 0 | extra |
