/// Calculator performs arithmetic operations.
pub struct Calculator;

impl Calculator {
    /// Returns the sum of two integers.
    pub fn add(&self, a: i64, b: i64) -> i64 {
        a + b
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        let calc = Calculator;
        assert_eq!(calc.add(2, 3), 5);
    }

    #[test]
    fn test_add_negative() {
        let calc = Calculator;
        assert_eq!(calc.add(2, -3), -1);
    }
}
