# Branching

score := 72
state := "paused"

grade := if(score >= 90, "A", score >= 80, "B", score >= 70, "C", "F")
icon := match(state, "running", "▸", "paused", "‖", "done", "✓", "○")

Grade [grade], icon [icon].
