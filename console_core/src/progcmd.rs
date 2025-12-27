// console_core/src/progcmd.rs
use crate::Programmer;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgWord {
    Num(u32),
    Thru,
    At,
    Full,
    Out,
    Percent(u8),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyStatus {
    Applied,
    Incomplete,    // valid so far, but needs more tokens
    NotProgrammer, // doesn't look like programmer syntax
}

fn lex(input: &str) -> Vec<String> {
    input.split_whitespace().map(|s| s.to_string()).collect()
}

fn parse_words(tokens: &[String]) -> Result<Vec<ProgWord>, ApplyStatus> {
    if tokens.is_empty() {
        return Err(ApplyStatus::NotProgrammer);
    }

    // If it doesn't start with a number, we treat it as "not our syntax"
    if tokens[0].parse::<u32>().is_err() {
        return Err(ApplyStatus::NotProgrammer);
    }

    let mut out = Vec::new();
    for t in tokens {
        let low = t.to_lowercase();
        let w = match low.as_str() {
            "thru" => ProgWord::Thru,
            "@" => ProgWord::At,
            "full" => ProgWord::Full,
            "out" => ProgWord::Out,
            _ => {
                if let Ok(n) = low.parse::<u32>() {
                    ProgWord::Num(n)
                } else if let Ok(p) = low.parse::<u8>() {
                    ProgWord::Percent(p.min(100))
                } else {
                    // Unknown token in programmer mode -> treat as "not our syntax"
                    return Err(ApplyStatus::NotProgrammer);
                }
            }
        };
        out.push(w);
    }

    Ok(out)
}

pub fn try_apply_programmer_line(line: &str, p: &mut Programmer) -> ApplyStatus {
    let tokens = lex(line);
    let words = match parse_words(&tokens) {
        Ok(w) => w,
        Err(status) => return status,
    };

    // Grammar (MVP):
    // <a>
    // <a> thru <b>
    // (optional) @ full|out|<0..100>
    //
    // Examples:
    // 101
    // 101 thru 105
    // 101 thru 105 @ full

    // Parse selection
    let mut i = 0;

    let a = match words.get(i) {
        Some(ProgWord::Num(n)) => *n,
        _ => return ApplyStatus::NotProgrammer,
    };
    i += 1;

    let (sel_a, sel_b) = match words.get(i) {
        Some(ProgWord::Thru) => {
            i += 1;
            let b = match words.get(i) {
                Some(ProgWord::Num(n)) => *n,
                _ => return ApplyStatus::Incomplete, // "101 thru" (waiting for end)
            };
            i += 1;
            (a, b)
        }
        _ => (a, a),
    };

    // Apply selection immediately
    p.selected.clear();
    if sel_a == sel_b {
        p.select_one(sel_a);
    } else {
        p.select_range(sel_a, sel_b);
    }

    // Optional: @ ...
    match words.get(i) {
        None => return ApplyStatus::Applied,
        Some(ProgWord::At) => {
            i += 1;
            let val = match words.get(i) {
                Some(ProgWord::Full) => Some(100),
                Some(ProgWord::Out) => Some(0),
                Some(ProgWord::Num(n)) if *n <= 100 => Some(*n as u8),
                _ => return ApplyStatus::Incomplete, // "101 @"
            };

            if let Some(pct) = val {
                p.set_intensity_percent(pct);
            }

            ApplyStatus::Applied
        }
        _ => ApplyStatus::Applied,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_range_full() {
        let mut p = Programmer::new();
        let st = try_apply_programmer_line("101 thru 105 @ full", &mut p);
        assert_eq!(st, ApplyStatus::Applied);
        assert!(p.selected.contains(&101));
        assert!(p.selected.contains(&105));
        assert_eq!(p.intensity, Some(255));
    }

    #[test]
    fn incomplete_thru_is_incomplete() {
        let mut p = Programmer::new();
        let st = try_apply_programmer_line("101 thru", &mut p);
        assert_eq!(st, ApplyStatus::Incomplete);
    }

    #[test]
    fn non_programmer_lines_are_ignored() {
        let mut p = Programmer::new();
        let st = try_apply_programmer_line("help", &mut p);
        assert_eq!(st, ApplyStatus::NotProgrammer);
    }

    #[test]
    fn parses_at_50() {
        let mut p = Programmer::new();
        let st = try_apply_programmer_line("1 thru 2 @ 50", &mut p);
        assert_eq!(st, ApplyStatus::Applied);
        assert!(p.selected.contains(&1));
        assert!(p.selected.contains(&2));
    }
}
