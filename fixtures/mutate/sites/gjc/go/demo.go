package demo
func Run(a int, b int) int {
    if a > 0 && b == 1 {
        return a * b
    }
    return a / b
}
type Widget struct{}
func (w Widget) Run() bool { return true }
