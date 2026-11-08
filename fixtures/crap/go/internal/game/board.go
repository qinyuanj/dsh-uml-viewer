package game

type Board struct{}

func (b *Board) Place(ready, ok bool) bool {
	if ready && ok {
		return true
	}
	return false
}
