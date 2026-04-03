use uuid::Uuid;

/// Generate an 8-character lowercase hex string from UUID4.
pub fn generate_id() -> String {
    Uuid::new_v4().as_simple().to_string()[..8].to_string()
}

/// CLI entry point — prints the ID to stdout.
pub fn run() {
    println!("{}", generate_id());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_8_chars() {
        let result = generate_id();
        assert_eq!(result.len(), 8);
    }

    #[test]
    fn is_lowercase_hex() {
        let result = generate_id();
        assert!(
            result.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
            "Not valid lowercase hex: {}",
            result
        );
    }

    #[test]
    fn two_calls_produce_different_values() {
        let a = generate_id();
        let b = generate_id();
        assert_ne!(a, b);
    }
}
