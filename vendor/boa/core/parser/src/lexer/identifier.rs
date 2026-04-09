//! This module implements lexing for identifiers (foo, myvar, etc.) used in ECMAScript.

use crate::lexer::{
    Cursor, Error, StringLiteral, Token, TokenKind, Tokenizer, token::ContainsEscapeSequence,
};
use crate::source::ReadChar;
use boa_ast::PositionGroup;
use boa_interner::Interner;
use icu_properties::props::{IdContinue, IdStart};
use icu_properties::{CodePointSetData, CodePointSetDataBorrowed};

const SUPPLEMENTAL_ID_START_RANGES: &[(u32, u32)] = &[
    (0x088F, 0x088F),
    (0x0C5C, 0x0C5C),
    (0x0CDC, 0x0CDC),
    (0xA7CE, 0xA7CF),
    (0xA7D2, 0xA7D2),
    (0xA7D4, 0xA7D4),
    (0xA7F1, 0xA7F1),
    (0x10940, 0x10959),
    (0x10EC5, 0x10EC7),
    (0x11DB0, 0x11DDB),
    (0x16EA0, 0x16EB8),
    (0x16EBB, 0x16ED3),
    (0x16FF2, 0x16FF6),
    (0x187F8, 0x187FF),
    (0x18D09, 0x18D1E),
    (0x18D80, 0x18DF2),
    (0x1E6C0, 0x1E6DE),
    (0x1E6E0, 0x1E6E2),
    (0x1E6E4, 0x1E6E5),
    (0x1E6E7, 0x1E6ED),
    (0x1E6F0, 0x1E6F4),
    (0x1E6FE, 0x1E6FF),
    (0x2B73A, 0x2B73F),
    (0x2CEA2, 0x2CEAD),
    (0x323B0, 0x33479),
];

const SUPPLEMENTAL_ID_CONTINUE_ONLY_RANGES: &[(u32, u32)] = &[
    (0x1ACF, 0x1ADD),
    (0x1AE0, 0x1AEB),
    (0x10EFA, 0x10EFB),
    (0x11B60, 0x11B67),
    (0x11DE0, 0x11DE9),
    (0x1E6E3, 0x1E6E3),
    (0x1E6E6, 0x1E6E6),
    (0x1E6EE, 0x1E6EF),
    (0x1E6F5, 0x1E6F5),
];

fn in_supplemental_ranges(ch: u32, ranges: &[(u32, u32)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| start <= ch && ch <= end)
}
/// Identifier lexing.
///
/// More information:
///  - [ECMAScript reference][spec]
///  - [MDN documentation][mdn]
///
/// [spec]: https://tc39.es/ecma262/#prod-Identifier
/// [mdn]: https://developer.mozilla.org/en-US/docs/Glossary/Identifier
#[derive(Debug, Clone, Copy)]
pub(super) struct Identifier {
    init: char,
}

impl Identifier {
    /// Creates a new identifier/keyword lexer.
    pub(super) const fn new(init: char) -> Self {
        Self { init }
    }

    /// Checks if a character is `IdentifierStart` as per ECMAScript standards.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-names-and-keywords
    pub(super) fn is_identifier_start(ch: u32) -> bool {
        const ID_START: CodePointSetDataBorrowed<'static> = CodePointSetData::new::<IdStart>();
        matches!(ch, 0x0024 /* $ */ | 0x005F /* _ */)
            || ID_START.contains32(ch)
            || in_supplemental_ranges(ch, SUPPLEMENTAL_ID_START_RANGES)
    }

    /// Checks if a character is `IdentifierPart` as per ECMAScript standards.
    ///
    /// More information:
    ///  - [ECMAScript reference][spec]
    ///
    /// [spec]: https://tc39.es/ecma262/#sec-names-and-keywords
    fn is_identifier_part(ch: u32) -> bool {
        const ID_CONTINUE: CodePointSetDataBorrowed<'static> =
            CodePointSetData::new::<IdContinue>();
        matches!(
            ch,
            0x0024 /* $ */ | 0x005F /* _ */ | 0x200C /* <ZWNJ> */ | 0x200D /* <ZWJ> */
        ) || ID_CONTINUE.contains32(ch)
            || Self::is_identifier_start(ch)
            || in_supplemental_ranges(ch, SUPPLEMENTAL_ID_CONTINUE_ONLY_RANGES)
    }
}

impl<R> Tokenizer<R> for Identifier {
    fn lex(
        &mut self,
        cursor: &mut Cursor<R>,
        start_pos: PositionGroup,
        interner: &mut Interner,
    ) -> Result<Token, Error>
    where
        R: ReadChar,
    {
        let (identifier_name, contains_escaped_chars) =
            Self::take_identifier_name(cursor, start_pos, self.init)?;

        let token_kind = match identifier_name.parse() {
            Ok(keyword) => TokenKind::Keyword((keyword, contains_escaped_chars)),
            Err(_) if identifier_name == "true" => {
                TokenKind::BooleanLiteral((true, ContainsEscapeSequence(contains_escaped_chars)))
            }
            Err(_) if identifier_name == "false" => {
                TokenKind::BooleanLiteral((false, ContainsEscapeSequence(contains_escaped_chars)))
            }
            Err(_) if identifier_name == "null" => {
                TokenKind::NullLiteral(ContainsEscapeSequence(contains_escaped_chars))
            }
            Err(_) => TokenKind::IdentifierName((
                interner.get_or_intern(identifier_name.as_str()),
                ContainsEscapeSequence(contains_escaped_chars),
            )),
        };

        Ok(Token::new_by_position_group(
            token_kind,
            start_pos,
            cursor.pos_group(),
        ))
    }
}

impl Identifier {
    pub(super) fn take_identifier_name<R>(
        cursor: &mut Cursor<R>,
        start_pos: PositionGroup,
        init: char,
    ) -> Result<(String, bool), Error>
    where
        R: ReadChar,
    {
        let mut contains_escaped_chars = false;
        let mut identifier_name = if init == '\\' && cursor.next_if(0x75 /* u */)? {
            let ch = StringLiteral::take_unicode_escape_sequence(cursor, start_pos.position())?;

            if Self::is_identifier_start(ch) {
                contains_escaped_chars = true;
                String::from(
                    char::try_from(ch)
                        .expect("all identifier starts must be convertible to strings"),
                )
            } else {
                return Err(Error::syntax("invalid identifier start", start_pos));
            }
        } else {
            // The caller guarantees that `init` is a valid identifier start
            String::from(init)
        };

        loop {
            let ch = match cursor.peek_char()? {
                Some(0x005C /* \ */) if cursor.peek_n(2)?[1] == Some(0x75) /* u */ => {
                    let pos = cursor.pos();
                    let _next = cursor.next_char();
                    let _next = cursor.next_char();
                    let ch = StringLiteral::take_unicode_escape_sequence(cursor, pos)?;

                    if Self::is_identifier_part(ch) {
                        contains_escaped_chars = true;
                        ch
                    } else {
                        return Err(Error::Syntax("invalid identifier part".into(), pos));
                    }
                }
                Some(ch) if Self::is_identifier_part(ch) => {
                    cursor.next_char()?;
                    ch
                },
                _ => break,
            };

            identifier_name.push(char::try_from(ch).expect("checked character value"));
        }

        Ok((identifier_name, contains_escaped_chars))
    }
}
