use std::fmt::Write;
use std::time::Duration;
use thiserror::Error;

const DEFAULT_PRECISION: usize = 6;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Format {
    pieces: Vec<FormatPiece>,
    newlines: usize,
}

impl Format {
    fn new() -> Format {
        Format {
            pieces: Vec::new(),
            newlines: 0,
        }
    }

    pub(crate) fn newlines(&self) -> usize {
        self.newlines
    }

    pub(crate) fn display(&self, d: Duration) -> String {
        let mut s = String::new();
        for p in &self.pieces {
            p.display(&mut s, d);
        }
        s
    }

    fn push_char(&mut self, c: char) {
        if let Some(FormatPiece::String(s)) = self.pieces.last_mut() {
            s.push(c);
        } else {
            self.pieces.push(FormatPiece::String(String::from(c)));
        }
        if c == '\n' {
            self.newlines += 1;
        }
    }

    fn push(&mut self, p: FormatPiece) {
        self.pieces.push(p);
    }
}

impl Default for Format {
    fn default() -> Format {
        Format {
            pieces: vec![
                FormatPiece::String("Elapsed: ".into()),
                FormatPiece::Hour,
                FormatPiece::String(":".into()),
                FormatPiece::Minute,
                FormatPiece::String(":".into()),
                FormatPiece::Second,
            ],
            newlines: 0,
        }
    }
}

impl std::str::FromStr for Format {
    type Err = ParseFormatError;

    fn from_str(s: &str) -> Result<Format, ParseFormatError> {
        let mut fmt = Format::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '%' => match chars.next() {
                    Some('H') => fmt.push(FormatPiece::Hour),
                    Some('M') => fmt.push(FormatPiece::Minute),
                    Some('S') => fmt.push(FormatPiece::Second),
                    Some('s') => fmt.push(FormatPiece::TotalSeconds),
                    Some('f') => fmt.push(FormatPiece::Subseconds {
                        precision: DEFAULT_PRECISION,
                    }),
                    Some('n') => fmt.push_char('\n'),
                    Some('t') => fmt.push_char('\t'),
                    Some('e') => fmt.push_char('\x1B'),
                    Some('%') => fmt.push_char('%'),
                    Some(c) if c.is_ascii_digit() => {
                        let mut precision = c.to_digit(10).expect("should be digit");
                        while let Some(c) = chars.next_if(char::is_ascii_digit) {
                            let d = c.to_digit(10).expect("should be digit");
                            precision = precision
                                .checked_mul(10)
                                .and_then(|p| p.checked_add(d))
                                .ok_or(ParseFormatError::PrecisionOverflow)?;
                        }
                        if chars.next() == Some('f') {
                            let precision = usize::try_from(precision)
                                .map_err(|_| ParseFormatError::PrecisionOverflow)?;
                            fmt.push(FormatPiece::Subseconds { precision });
                        } else {
                            return Err(ParseFormatError::InvalidPercent(c));
                        }
                    }
                    Some(c) => return Err(ParseFormatError::InvalidPercent(c)),
                    None => return Err(ParseFormatError::BrokenPercent),
                },
                '\\' => match chars.next() {
                    Some('n') => fmt.push_char('\n'),
                    Some('t') => fmt.push_char('\t'),
                    Some('e') => fmt.push_char('\x1B'),
                    Some('\\') => fmt.push_char('\\'),
                    Some(c) => return Err(ParseFormatError::InvalidEscape(c)),
                    None => return Err(ParseFormatError::BrokenEscape),
                },
                c => fmt.push_char(c),
            }
        }
        Ok(fmt)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum FormatPiece {
    String(String),
    Hour,
    Minute,
    Second,
    TotalSeconds,
    Subseconds { precision: usize },
}

impl FormatPiece {
    fn display(&self, out: &mut String, d: Duration) {
        match self {
            FormatPiece::String(s) => out.push_str(s),
            FormatPiece::Hour => {
                let _ = write!(out, "{:02}", d.as_secs() / 3600);
            }
            FormatPiece::Minute => {
                let _ = write!(out, "{:02}", d.as_secs() / 60 % 60);
            }
            FormatPiece::Second => {
                let _ = write!(out, "{:02}", d.as_secs() % 60);
            }
            FormatPiece::TotalSeconds => {
                let _ = write!(out, "{}", d.as_secs());
            }
            FormatPiece::Subseconds { precision } => {
                let mut frac = d.subsec_nanos();
                let mut divisor = 1_000_000_000 / 10;
                for _ in 0..*precision {
                    let d;
                    if divisor > 0 {
                        d = frac / divisor;
                        frac %= divisor;
                        divisor /= 10;
                    } else {
                        d = 0;
                    }
                    // Don't bother trying to round up, as doing that
                    // correctly would mean sometimes incrementing every
                    // higher time component as well.
                    out.push(char::from_digit(d, 10).expect("should be valid decimal digit"));
                }
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub(crate) enum ParseFormatError {
    #[error("numeric overflow while parsing %f precision")]
    PrecisionOverflow,
    #[error("'%' followed by invalid specifier {0:?}")]
    InvalidPercent(char),
    #[error("'%' not followed by anything")]
    BrokenPercent,
    #[error("backslash followed by invalid character {0:?}")]
    InvalidEscape(char),
    #[error("backslash not followed by anything")]
    BrokenEscape,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn parse_default() {
        let fmt = "Elapsed: %H:%M:%S".parse::<Format>().unwrap();
        assert_eq!(fmt, Format::default());
    }

    #[test]
    fn newlines() {
        let fmt = "Hours: %H%nMinutes: %M\\nSeconds: %S\n"
            .parse::<Format>()
            .unwrap();
        assert_eq!(fmt.newlines(), 3);
    }

    #[rstest]
    #[case("Elapsed: %H:%M:%S", Duration::ZERO, "Elapsed: 00:00:00")]
    #[case("", Duration::ZERO, "")]
    #[case("Elapsed: %H:%M:%S.%f", Duration::ZERO, "Elapsed: 00:00:00.000000")]
    #[case("Elapsed: %s.%f", Duration::ZERO, "Elapsed: 0.000000")]
    #[case("Elapsed: %s.%0f", Duration::ZERO, "Elapsed: 0.")]
    #[case("Elapsed: %s.%1f", Duration::ZERO, "Elapsed: 0.0")]
    #[case("Elapsed: %H:%M:%S", Duration::from_secs(2 * 3600 + 34 * 60 + 56), "Elapsed: 02:34:56")]
    #[case("Elapsed: %s", Duration::from_secs(2 * 3600 + 34 * 60 + 56), "Elapsed: 9296")]
    #[case("Elapsed: %s.%2f", Duration::from_millis(123), "Elapsed: 0.12")]
    #[case("Elapsed: %s.%2f", Duration::from_millis(999), "Elapsed: 0.99")]
    #[case("Elapsed: %s.%f", Duration::from_nanos(123456789), "Elapsed: 0.123456")]
    #[case(
        "Elapsed: %s.%20f",
        Duration::from_nanos(123456789),
        "Elapsed: 0.12345678900000000000"
    )]
    #[case(
        "/%%\\\\ %e[1mElapsed:\\e[m%t\\t%H:%M:%S",
        Duration::ZERO,
        "/%\\ \x1B[1mElapsed:\x1B[m\t\t00:00:00"
    )]
    fn display(#[case] spec: &str, #[case] d: Duration, #[case] out: &str) {
        let fmt = spec.parse::<Format>().unwrap();
        assert_eq!(fmt.display(d), out);
    }

    #[rstest]
    #[case("Years: %Y")]
    #[case("Years: %")]
    #[case("Time: %s\\r")]
    #[case("Time: %s\\")]
    #[case("Time: %s.%999999999999f")]
    #[case("Time: %s.%999_999f")]
    fn parse_err(#[case] spec: &str) {
        let r = spec.parse::<Format>();
        assert!(r.is_err());
    }
}
