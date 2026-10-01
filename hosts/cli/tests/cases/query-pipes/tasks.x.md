# Errands

- [ ] Call the bank @due(2026-09-20) #phone
- [x] Return the parcel @due(2026-09-17)
- [ ] Book the dentist @due(2026-09-18) #phone
- [ ] Water the plants

open := tasks_left
  | filter(fn(n) => n > 1)
  | length

tasks_left := [1, 2, 3]

Open [open]

sizes := table
| item  | count                 |
| ----- | --------------------- |
| short | [tasks_left \| length] |

Sizes [sizes]
