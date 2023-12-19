extern crate logos;

use crate::{syntax::SyntaxKind, CommandSpec};
use logos::Logos;

/// A peeked token
type PeekTok<'a> = (Token, &'a str);

/// Small memory-efficient lexer for TeX
///
/// It gets improved performance on x86_64 but not wasm through
#[derive(Debug, Clone)]
pub struct Lexer<'a> {
    /// The inner lexer
    inner: logos::Lexer<'a, Token>,
    /// The last peeked token
    peeked: Option<PeekTok<'a>>,
    /// A set of peeked tokens takes up to one page of memory
    /// It also takes CPU locality into consideration
    peek_cache: Vec<PeekTok<'a>>,
}

impl<'a> Lexer<'a> {
    /// Create a new lexer
    pub fn new(input: &'a str, spec: CommandSpec) -> Self {
        let inner = Token::lexer_with_extras(input, spec);
        let mut n = Self {
            inner,
            peeked: None,
            peek_cache: Vec::with_capacity(16),
        };
        n.next();

        n
    }

    /// Private method to fill the peek cache with a page of tokens at the same
    /// time
    fn bump_batched(&mut self) {
        /// The size of a page, in some architectures it is 16384B but that
        /// doesn't matter
        const PAGE_SIZE: usize = 4096;
        /// The item size of the peek cache
        const PEEK_CACHE_SIZE: usize = (PAGE_SIZE - 16) / std::mem::size_of::<PeekTok<'static>>();

        for _ in 0..PEEK_CACHE_SIZE {
            let kind = self.inner.next().map(|token| {
                let kind = token.unwrap();
                let text = self.inner.slice();
                if kind == Token::CommandName(CommandName::Generic) {
                    let name = classify(&text[1..]);
                    (Token::CommandName(name), text)
                } else {
                    (kind, text)
                }
            });
            if let Some(kind) = kind {
                self.peek_cache.push(kind);
            } else {
                break;
            }
        }
        // Reverse the peek cache to make it a stack
        self.peek_cache.reverse();
    }

    /// Private method to advance the lexer
    #[inline]
    fn next(&mut self) {
        if let Some(peeked) = self.peek_cache.pop() {
            self.peeked = Some(peeked);
            return;
        }

        // it is not likely to be inlined
        self.bump_batched();

        // Pop the first token again
        self.peeked = self.peek_cache.pop();
    }

    /// Peek the next token
    pub fn peek(&self) -> Option<Token> {
        self.peeked.map(|(kind, _)| kind)
    }

    /// Peek the next token's text
    pub fn peek_text(&self) -> Option<&'a str> {
        self.peeked.map(|(_, text)| text)
    }

    pub fn peek_char(&self) -> Option<char> {
        self.peek_text().map(str::chars).and_then(|mut e| e.next())
    }

    /// Update the text part of the peeked token
    pub fn consume_word(&mut self, cnt: usize) {
        let Some(peek_mut) = &mut self.peeked else {
            return;
        };
        if peek_mut.1.len() <= cnt {
            self.next();
        } else {
            peek_mut.1 = &peek_mut.1[cnt..];
        }
    }

    /// Update the peeked token and return the old one
    pub fn eat(&mut self) -> Option<(SyntaxKind, &'a str)> {
        let (kind, text) = self.peeked.take()?;
        self.next();
        Some((kind.into(), text))
    }
}

/// Classify the command name so parser can use it repeatedly
fn classify(name: &str) -> CommandName {
    match name {
        "begin" => CommandName::BeginEnvironment,
        "end" => CommandName::EndEnvironment,
        "iffalse" => CommandName::BeginBlockComment,
        "fi" => CommandName::EndBlockComment,
        "left" => CommandName::Left,
        "right" => CommandName::Right,
        _ => CommandName::Generic,
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub enum BraceKind {
    Curly,
    Bracket,
    Paren,
}

#[inline(always)]
fn bc(_: &mut logos::Lexer<Token>) -> BraceKind {
    BraceKind::Curly
}

#[inline(always)]
fn bb(_: &mut logos::Lexer<Token>) -> BraceKind {
    BraceKind::Bracket
}

#[inline(always)]
fn bp(_: &mut logos::Lexer<Token>) -> BraceKind {
    BraceKind::Paren
}

/// The token type defined by logos
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash, Logos)]
#[logos(extras = CommandSpec)]
pub enum Token {
    #[regex(r"[\r\n]+", priority = 2)]
    LineBreak,

    #[regex(r"[^\S\r\n]+", priority = 1)]
    Whitespace,

    #[regex(r"%[^\r\n]*")]
    LineComment,

    #[token("{", bc)]
    #[token("[", bb)]
    #[token("(", bp)]
    Left(BraceKind),

    #[token("}", bc)]
    #[token("]", bb)]
    #[token(")", bp)]
    Right(BraceKind),

    #[token(",")]
    Comma,

    #[token("~")]
    Tilde,

    #[token("/")]
    Divide,

    #[token("=")]
    Equal,

    #[token("&")]
    And,

    #[token("^")]
    Caret,

    #[token("'")]
    Apostrophe,

    #[token("_", priority = 2)]
    Underline,

    #[regex(r"[^\s\\%\{\},\$\[\]\(\)\~/=_'^]+", priority = 1)]
    Word,

    #[regex(r"\$\$?")]
    Dollar,

    #[regex(r"\\\\", priority = 4)]
    NewLine,

    #[regex(r"\\", lex_command_name, priority = 3)]
    CommandName(CommandName),
}

/// Lex the command name
fn lex_command_name(lexer: &mut logos::Lexer<Token>) -> CommandName {
    let mut chars = lexer.source()[lexer.span().end..].chars();

    let Some(c) = chars.next() else {
        return CommandName::Generic;
    };

    // Case1: `\ ` is not a command name hence the command is empty
    // Note: a space is not a command name
    if c.is_whitespace() {
        return CommandName::Generic;
    }

    // Case2: `\.*` is a command name, e.g. `\;` is a space command in TeX
    // Note: the first char is always legal, since a backslash with any single char
    // is a valid escape sequence
    lexer.bump(c.len_utf8());
    // Lex the command name if it is not an escape sequence
    if !c.is_alphanumeric() && c != '@' {
        return CommandName::Generic;
    }

    for c in chars {
        match c {
            '*' => {
                const LEN_ASK: usize = 1;
                lexer.bump(LEN_ASK);
                break;
            }
            c if c.is_alphanumeric() => {
                const LEN_WORD: usize = 1;
                lexer.bump(LEN_WORD);
            }
            '@' | ':' => {
                const LEN_SPECIAL: usize = 1;
                lexer.bump(LEN_SPECIAL);
            }
            _ => {
                break;
            }
        };
    }

    CommandName::Generic
}

/// The command name used by parser
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub enum CommandName {
    /// Rest of the command names
    Generic,
    /// clause of Environment: \begin
    BeginEnvironment,
    /// clause of Environment: \end
    EndEnvironment,
    /// clause of BlockComment: \iffalse
    BeginBlockComment,
    /// clause of BlockComment: \fi
    EndBlockComment,
    /// clause of LRItem: \left
    Left,
    /// clause of LRItem: \right
    Right,
}
