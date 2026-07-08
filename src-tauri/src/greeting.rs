pub fn greet(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        "Hello, stranger!".to_string()
    } else {
        format!("Hello, {name}!")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greets_by_name() {
        assert_eq!(greet("Domco"), "Hello, Domco!");
    }

    #[test]
    fn trims_surrounding_whitespace() {
        assert_eq!(greet("  Domco  "), "Hello, Domco!");
    }

    #[test]
    fn falls_back_for_empty_input() {
        assert_eq!(greet(""), "Hello, stranger!");
        assert_eq!(greet("   "), "Hello, stranger!");
    }
}
