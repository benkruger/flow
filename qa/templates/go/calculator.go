package calculator

// Calculator performs arithmetic operations.
type Calculator struct{}

// Add returns the sum of two integers.
func (c Calculator) Add(a, b int) int {
	return a + b
}
