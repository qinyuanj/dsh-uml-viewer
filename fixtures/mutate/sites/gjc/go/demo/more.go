package demo

import "fmt"

func Ops(a, b int, ok bool) (int, bool) {
	x := -a + +b - 1
	y := a*b - a/b + a%b
	z := a &^ b
	if !ok || a != b || a <= b || a >= b || a < 0 {
		x++
	}
	c := make(chan int, 1)
	c <- 0
	p := &x
	*p = <-c
	s := "a + b 1"
	r := 'x'
	f := 1.0 + 0.5
	h := 0x1 + 01 + 1_0
	fmt.Println(s, r, f, h, y, z, true, false, nil)
	return x, !ok
}

func _private() int { return 0 }

func Éxported() int { return 1 }

func (w *Widget) ptr(v int) int { return v * 1 }

type Widget struct{ n int }

var top = 1 + 0
