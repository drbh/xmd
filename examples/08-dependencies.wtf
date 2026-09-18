# Task dependencies

@after blocks a task until every dependency is done.

- [x] Order parts :parts
- [ ] Assemble :assemble @after(parts)
- [ ] Test @after(assemble)
- [ ] Ship @after(assemble, budget_ok)

$500:budget
$420:spent
budget_ok := spent <= budget

<!-- Blocked tasks say what they wait for. Cycles are reported with locations.
Try: complete Assemble and watch Test and Ship unblock. -->
