package calculator

import "testing"

func TestAdd(t *testing.T) {
	calc := Calculator{}
	got := calc.Add(2, 3)
	if got != 5 {
		t.Errorf("Add(2, 3) = %d, want 5", got)
	}
}

func TestAddNegative(t *testing.T) {
	calc := Calculator{}
	got := calc.Add(2, -3)
	if got != -1 {
		t.Errorf("Add(2, -3) = %d, want -1", got)
	}
}
