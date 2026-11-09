package widgets

import "testing"

func TestArea(t *testing.T) {
	if Area(2, 3) != 6 {
		t.Fatal("area")
	}
}

func TestEmpty(t *testing.T) {
	if !(Box{}).Empty() || (Box{Size: 2}).Empty() {
		t.Fatal("empty")
	}
}
