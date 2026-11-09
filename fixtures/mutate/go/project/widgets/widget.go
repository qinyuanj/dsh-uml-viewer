package widgets

type Box struct{ Size int }

func Area(w, h int) int {
	return w * h
}

func (b Box) Empty() bool {
	return b.Size == 0
}
