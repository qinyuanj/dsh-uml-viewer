package demo

import "testing"

func TestClamp(t *testing.T) {
	if Clamp(5, 0, 10) != 5 || Clamp(-1, 0, 10) != 0 || Clamp(11, 0, 10) != 10 {
		t.Fatal("clamp")
	}
}

func TestDouble(t *testing.T) {
	if Double(3) != 6 {
		t.Fatal("double")
	}
}

func TestEven(t *testing.T) {
	if !isEven(4) || isEven(3) {
		t.Fatal("even")
	}
}

func TestReady(t *testing.T) {
	if !Ready(false) {
		t.Fatal("ready")
	}
}

func TestHalve(t *testing.T) {
	if Halve(8) != 4 {
		t.Fatal("halve")
	}
}

func TestCounter(t *testing.T) {
	var c Counter
	if c.Add(2) != 2 {
		t.Fatal("add")
	}
}
