package board

func total(xs []int) []int {
	ys := filter(xs, odd)
	zs := mapFn(ys, inc)
	return list(zs)
}

func (b *Board) run(items []int) []int {
	kept := filter(items, even)
	out := mapFn(kept, dec)
	return list(out)
}

func declared(xs []int)
