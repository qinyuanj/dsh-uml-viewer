package sample

func Branchy(x int) int {
	if x > 0 && x < 10 {
		return 1
	}
	switch x {
	case 1, 2:
		return 2
	default:
		return 3
	}
}

type Widget struct{}

func (w *Widget) Run(items []int) int {
	total := 0
	for i := 0; i < 10; i++ {
		total += i
	}
	for _, item := range items {
		total += item
	}
	return total
}

func (w Widget) Ready(ready, ok bool) bool {
	if ready || ok {
		return true
	}
	return false
}

func Simple() int { return 1 }

func Closure(x int) int {
	choose := func() int {
		if x > 0 {
			return x
		}
		return 0
	}
	return choose()
}

func Cases(value any, ch chan int) int {
	switch value.(type) {
	case int:
		return 1
	default:
		select {
		case n := <-ch:
			return n
		default:
			return 0
		}
	}
}
