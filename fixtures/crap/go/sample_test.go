package sample

import "testing"

func TestFunctions(t *testing.T) {
	if Branchy(3) != 1 || Branchy(20) != 3 {
		t.Fatal("branch selection")
	}
	widget := Widget{}
	if widget.Run([]int{2, 3}) != 50 {
		t.Fatal("loop totals")
	}
	if !widget.Ready(false, true) || widget.Ready(false, false) {
		t.Fatal("value receiver")
	}
	if Simple() != 1 || Closure(2) != 2 || Closure(-1) != 0 {
		t.Fatal("simple and closure")
	}
	ch := make(chan int, 1)
	ch <- 7
	if Cases(1, ch) != 1 || Cases("value", ch) != 7 || Cases(nil, ch) != 0 {
		t.Fatal("type switch and select")
	}
}
