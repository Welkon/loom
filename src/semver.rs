//! Small semver comparison and requirement matching.

pub(super) fn is_newer(candidate: &str, current: &str) -> bool {
    parse(candidate) > parse(current)
}

fn parse(value: &str) -> Option<(u64, u64, u64)> {
    let core = value.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

pub(super) fn satisfies(version: &str, requirement: &str) -> bool {
    let Some(current) = parse(version) else {
        return false;
    };
    let requirement = requirement.trim();
    if requirement.is_empty() || requirement == "*" {
        return true;
    }
    let (operator, value) = if let Some(rest) = requirement.strip_prefix(">=") {
        (">=", rest)
    } else if let Some(rest) = requirement.strip_prefix("<=") {
        ("<=", rest)
    } else if let Some(rest) = requirement.strip_prefix('>') {
        (">", rest)
    } else if let Some(rest) = requirement.strip_prefix('<') {
        ("<", rest)
    } else if let Some(rest) = requirement.strip_prefix('^') {
        ("^", rest)
    } else if let Some(rest) = requirement.strip_prefix('~') {
        ("~", rest)
    } else if let Some(rest) = requirement.strip_prefix('=') {
        ("=", rest)
    } else {
        ("=", requirement)
    };
    let Some(target) = parse(value) else {
        return false;
    };
    match operator {
        ">=" => current >= target,
        "<=" => current <= target,
        ">" => current > target,
        "<" => current < target,
        "^" => {
            current >= target && {
                if target.0 > 0 {
                    current.0 == target.0
                } else if target.1 > 0 {
                    current.0 == 0 && current.1 == target.1
                } else {
                    current == target
                }
            }
        }
        "~" => current >= target && current.0 == target.0 && current.1 == target.1,
        _ => current == target,
    }
}

#[cfg(test)]
mod tests {
    use super::satisfies;

    #[test]
    fn matches_caret_comparator_and_exact_ranges() {
        assert!(satisfies("0.1.5", "^0.1.0"));
        assert!(!satisfies("0.2.0", "^0.1.0"));
        assert!(satisfies("1.2.0", "^1.0.0"));
        assert!(!satisfies("2.0.0", "^1.0.0"));
        assert!(satisfies("1.2.9", "~1.2.0"));
        assert!(!satisfies("1.3.0", "~1.2.0"));
        assert!(satisfies("2.0.0", ">=1.5.0"));
        assert!(!satisfies("1.0.0", ">1.0.0"));
        assert!(satisfies("1.0.0", "1.0.0"));
    }
}
