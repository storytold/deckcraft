pub(super) fn pretty_equation(s: &str) -> String {
    let sup = |c: char| match c {
        '0' => '⁰',
        '1' => '¹',
        '2' => '²',
        '3' => '³',
        '4' => '⁴',
        '5' => '⁵',
        '6' => '⁶',
        '7' => '⁷',
        '8' => '⁸',
        '9' => '⁹',
        'n' => 'ⁿ',
        'i' => 'ⁱ',
        '+' => '⁺',
        '-' => '⁻',
        _ => c,
    };
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '^' => {
                while let Some(&n) = chars.peek() {
                    if n.is_ascii_alphanumeric() || n == '+' || n == '-' {
                        out.push(sup(n));
                        chars.next();
                        if !n.is_ascii_digit() {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }
            '*' => out.push('·'),
            _ => out.push(c),
        }
    }
    out.replace("sqrt", "√").replace("<=", "≤").replace(">=", "≥").replace("!=", "≠").replace("pi", "π").replace("+-", "±")
}
