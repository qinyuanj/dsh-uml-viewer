package demo

// Clamp keeps x inside [lo, hi].
func Clamp(x, lo, hi int) int {
	if x < lo {
		return lo
	}
	if x > hi {
		return hi
	}
	return x
}

func Double(x int) int {
	return x * 2
}

func isEven(n int) bool {
	return n%2 == 0
}

func Ready(flag bool) bool {
	return !flag && true
}

func Halve(x int) int {
	return x / 2
}

func Untested(a int) int {
	return a + 1
}

type Counter struct{ n int }

func (c *Counter) Add(step int) int {
	c.n = c.n + step
	return c.n
}
