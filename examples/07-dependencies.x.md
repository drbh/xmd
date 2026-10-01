# Dependencies

- [x] Order parts :parts
- [ ] Assemble :assemble @after(parts)
- [ ] Test @after(assemble)
- [ ] Ship @after(assemble, budget_ok)

$500:budget
$420:spent
budget_ok := spent <= budget
