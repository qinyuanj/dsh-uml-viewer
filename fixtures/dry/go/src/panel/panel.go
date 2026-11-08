package panel

func sum(xs []int) []int {
	ys := filter(xs, odd)
	zs := mapFn(ys, inc)
	return list(zs)
}

func (p *Panel) walk(items []int) []int {
	kept := filter(items, even)
	out := mapFn(kept, dec)
	return list(out)
}
