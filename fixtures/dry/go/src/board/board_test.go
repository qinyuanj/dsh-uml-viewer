package board

func totalCopy(xs []int) []int {
	ys := filter(xs, odd)
	zs := mapFn(ys, inc)
	return list(zs)
}
