package game

import "testing"

func TestPlace(t *testing.T) {
	board := Board{}
	if !board.Place(true, true) || board.Place(false, true) {
		t.Fatal("board placement")
	}
}
