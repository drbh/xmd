# Goal seek

$1,200:saved
9:months_left
saved_by_june := monthly * months_left + saved
monthly := solve(saved_by_june >= $5,000)

Save [monthly] a month to reach [saved_by_june] by June.
