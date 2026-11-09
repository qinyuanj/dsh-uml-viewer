package mix
func Mix(a int, b int) int { return ((a ^ b) & (a | b)) << ((a >> b) % b) }
